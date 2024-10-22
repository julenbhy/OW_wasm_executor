import argparse
import os
import subprocess
import re

import json
import requests
import time

import logging as log
import statistics
import csv
import pprint
from tabulate import tabulate
from datetime import timedelta

class Config:
    def __init__(self):
        self.num_runs = 1
        self.num_invocations = 100
        self.warmup_invocations = 1
        self.workers = 1
        self.verbose = False
        self.time_limit = 30
        self.time_precision = 1
        self.function = "noop"
        self.blocking = False
        self.input_file = None
        self.input_string = None
        self.output_file = None
        self.payload = json.loads('{"param1": "default", "param2": "payload"}')
        self.apihost = "http://172.17.0.1:3233/api/v1"
        self.authorization = "Basic MjNiYzQ2YjEtNzFmNi00ZWQ1LThjNTQtODE2YWE0ZjhjNTAyOjEyM3pPM3haQ0xyTU42djJCS0sxZFhZRnBYbFBrY2NPRnFtMTJDZEFzTWdSVTRWck5aOWx5R1ZDR3VNREdJd1A="

    def print_config(self):
        # Organizing configurations in a dictionary
        config = {
            "Number of runs": self.num_runs,
            "Number of invocations": self.num_invocations,
            "Warmup runs": self.warmup_invocations,
            "Workers": self.workers,
            "Verbose": self.verbose,
            "Function": self.function,
            "Blocking": self.blocking,
            "Input file": self.input_file,
            "Input string": self.input_string,
            "Output file": self.output_file,
            "Time limit (s)": self.time_limit,
            "Time precision (ms)": self.time_precision,
            "APIHOST": self.apihost,
            "Authorization": self.authorization
        }

        if self.payload is not None:
            try:
                config["Payload"] = json.dumps(self.payload, indent=4)
            except json.JSONDecodeError:
                config["Payload"] = str(self.payload)
        else:
            config["Payload"] = "None"

        # Printing configurations in a formatted way
        log.info("Configuration Parameters:")
        for key, value in config.items():
            log.info(f"{key}: {value}")
        log.info("\n\n")

    def parse_arguments(self):
        parser = argparse.ArgumentParser(description='Arguments for benchmarking')

        # Basic parameters
        parser.add_argument('-n', '--num-runs', type=int, default=self.num_runs, metavar='',
                            help=f'Number of runs for each benchmark (default: {self.num_runs})')
        
        parser.add_argument('-i', '--num-invocations', type=int, default=self.num_invocations, metavar='',
                            help=f'Number of invocations for each benchmark (default: {self.num_invocations})')
        
        parser.add_argument('-w', '--warmup_invocations', type=int, default=self.warmup_invocations, metavar='',
                            help=f'Number of warmup invocations (default: {self.warmup_invocations})')

        parser.add_argument('-W', '--workers', type=int, default=self.workers, metavar='',
                            help=f'Number of workers invoked por activation (for burst OpenWhisk) (default: {self.workers})')
        
        parser.add_argument('-f', '--function', type=str, default=self.function, metavar='',
                            help=f'Name of the function to benchmark (default: {self.function})')
        
        parser.add_argument('-b', '--blocking', action='store_true', default=self.blocking,
                    help='Enable blocking call (default: {})'.format(self.blocking))
        
        parser.add_argument('-t', '--time-limit', type=int, default=self.time_limit, metavar='',
                            help=f'Time limit for each benchmark (default: {self.time_limit} seconds)')

        parser.add_argument('-T', '--time-precision', type=int, default=self.time_precision, metavar='',
                            help=f'Time precision for measuring elapsed time (default: {self.time_precision} ms)')
        
        parser.add_argument('-v', '--verbose', action='store_true', default=self.verbose,
                            help='Enable verbose output (default: {})'.format(self.verbose))

        # Input-related parameters (mutually exclusive group)
        input_group = parser.add_mutually_exclusive_group()
        input_group.add_argument('-I', '--input_file', type=str, default=self.input_file, metavar='',
                                 help='Input file for the function (default: {})'.format(self.input_file))

        input_group.add_argument('-s', '--input_string', type=str, default=self.input_string, metavar='',
                                 help='Input string for the function (default: {})'.format(self.input_string))
                                
        parser.add_argument('-o', '--output_file', type=str, default=self.output_file, metavar='',
                            help='Output file for the function (default: {})'.format(self.output_file))

        # API-related parameters
        parser.add_argument('-A', '--apihost', type=str, default=self.apihost, metavar='',
                            help='APIHOST (default: {})'.format(self.apihost))

        parser.add_argument('-a', '--authorization', type=str, default=self.authorization, metavar='',
                            help='Authorization (default: {})'.format(self.authorization))

        args = parser.parse_args()

        # Updating configuration with parsed arguments
        self.num_runs = args.num_runs
        self.num_invocations = args.num_invocations
        self.warmup_invocations = args.warmup_invocations
        self.workers = args.workers
        self.function = args.function
        self.blocking = args.blocking
        self.input_file = args.input_file
        self.input_string = args.input_string
        self.output_file = args.output_file
        self.time_limit = args.time_limit
        self.time_precision = args.time_precision
        self.apihost = args.apihost
        self.authorization = args.authorization

        self.verbose = args.verbose
        if self.verbose: log.basicConfig(format='%(message)s', level=log.INFO)

        if self.input_file:
            self.payload = json.loads(open(self.input_file).read())
        elif self.input_string:
            self.payload = json.loads(self.input_string)


def extract_metrics(response, config):
    """
    Extracts relevant metrics from the get_response JSON content, including initTime, duration, client_elapsed_time, waitTime, and success.
    """
    try:
        json_response = response.json()
        annotations = {item['key']: item['value'] for item in json_response.get('annotations', [])}
        
        init_time = annotations.get('initTime', 0)
        wait_time = annotations.get('waitTime', 0)
        duration = json_response.get('duration', 0)
        client_elapsed_time = getattr(response, 'client_elapsed_time', 0)
        if config.blocking: success = json_response.get('success', False)
        else: success = json_response.get('response', {}).get('status', '').lower() == 'success'

        return {
            'initTime': init_time,
            'waitTime': wait_time,
            'duration': duration,
            'client_elapsed_time': client_elapsed_time,
            'success': success
        }
    except (ValueError, KeyError): return {'initTime': 0, 'waitTime': 0, 'duration': 0, 'client_elapsed_time': 0, 'success': False}


def benchmark_statistics(metrics_list):
    """
    Computes statistics for each metric.
    Returns average, minimum, and maximum for initTime, duration, and client_elapsed_time,
    and the overall success rate.
    """
    init_times = [m['initTime'] for m in metrics_list]
    wait_times = [m['waitTime'] for m in metrics_list]
    durations = [m['duration'] for m in metrics_list]
    client_elapsed_times = [m['client_elapsed_time'] for m in metrics_list]
    success_rate = sum(1 for m in metrics_list if m['success']) / len(metrics_list) * 100

    stats = {
        'initTime': {'avg': statistics.mean(init_times), 'min': min(init_times), 'max': max(init_times), 'std': statistics.stdev(init_times)},
        'waitTime': {'avg': statistics.mean(wait_times), 'min': min(wait_times), 'max': max(wait_times), 'std': statistics.stdev(wait_times)},
        'duration': {'avg': statistics.mean(durations), 'min': min(durations), 'max': max(durations), 'std': statistics.stdev(durations)},
        'client_elapsed_time': {'avg': statistics.mean(client_elapsed_times), 'min': min(client_elapsed_times), 'max': max(client_elapsed_times), 'std': statistics.stdev(client_elapsed_times)},
        'success_rate': success_rate
    }
    return stats


def format_results(stats, config):
    """
    Formats the benchmark results for display as a table.
    """
    headers = ["Metric", "Average", "Minimum", "Maximum", "Standard Deviation"]
    table_data = [
        ["InitTime", f"{stats['initTime']['avg']:.4f}", f"{stats['initTime']['min']:.4f}", f"{stats['initTime']['max']:.4f}", f"{stats['initTime']['std']:.4f}"],
        ["WaitTime", f"{stats['waitTime']['avg']:.4f}", f"{stats['waitTime']['min']:.4f}", f"{stats['waitTime']['max']:.4f}", f"{stats['waitTime']['std']:.4f}"],
        ["Duration", f"{stats['duration']['avg']:.4f}", f"{stats['duration']['min']:.4f}", f"{stats['duration']['max']:.4f}", f"{stats['duration']['std']:.4f}"],
        ["Client Elapsed Time", f"{stats['client_elapsed_time']['avg']:.4f}", f"{stats['client_elapsed_time']['min']:.4f}", f"{stats['client_elapsed_time']['max']:.4f}", f"{stats['client_elapsed_time']['std']:.4f}"],
        ["Success Rate", f"{stats['success_rate']:.2f}%", "-", "-", "-"]
    ]

    # Display the number of runs and invocations
    result = (
        f"\n\n-------------------------------------------------------------------"
        f"\nBenchmark Results:\n"
        f"Number of Warp-up invocations: {config.warmup_invocations}\n"
        f"Number of runs: {config.num_runs}\n"
        f"Number of invocations per run: {config.num_invocations}\n\n"
        f"{tabulate(table_data, headers=headers, tablefmt='grid')}\n"
    )
    return result


def write_results_to_file_csv(stats, config):
    """
    Writes the benchmark results to a specified CSV file.
    """
    with open(config.output_file, 'w', newline='') as csvfile:
        fieldnames = ['Metric', 'Average', 'Min', 'Max', 'Std', 'Success Rate']
        writer = csv.DictWriter(csvfile, fieldnames=fieldnames)

        # Write headers
        writer.writeheader()

        # Write rows for each metric
        writer.writerow({'Metric': 'InitTime', 'Average': f"{stats['initTime']['avg']:.4f}",
                         'Min': f"{stats['initTime']['min']:.4f}", 'Max': f"{stats['initTime']['max']:.4f}", 'Std': f"{stats['initTime']['std']:.4f}"})
        writer.writerow({'Metric': 'WaitTime', 'Average': f"{stats['waitTime']['avg']:.4f}",
                         'Min': f"{stats['waitTime']['min']:.4f}", 'Max': f"{stats['waitTime']['max']:.4f}", 'Std': f"{stats['waitTime']['std']:.4f}"})
        writer.writerow({'Metric': 'Duration', 'Average': f"{stats['duration']['avg']:.4f}",
                         'Min': f"{stats['duration']['min']:.4f}", 'Max': f"{stats['duration']['max']:.4f}", 'Std': f"{stats['duration']['std']:.4f}"})
        writer.writerow({'Metric': 'Client Elapsed Time', 'Average': f"{stats['client_elapsed_time']['avg']:.4f}",
                         'Min': f"{stats['client_elapsed_time']['min']:.4f}", 'Max': f"{stats['client_elapsed_time']['max']:.4f}", 'Std': f"{stats['client_elapsed_time']['std']:.4f}"})
        writer.writerow({'Metric': 'Success Rate', 'Average': f"{stats['success_rate']:.2f}%", 'Min': '', 'Max': ''})

    log.info(f"Results written to {config.output_file}")


def format_response_dict(response):
    """
    Format the __dict__ of a response, including decoding and formatting the '_content' field if it's JSON.
    """
    response_dict = response.__dict__.copy()

    # Try to decode and pretty-print the '_content' field if it contains JSON data
    if isinstance(response_dict.get('_content'), bytes):
        try:
            decoded_content = response_dict['_content'].decode('utf-8')  # Decode bytes to string
            json_content = json.loads(decoded_content)  # Parse string as JSON
            response_dict['_content'] = json_content  # Store as a dict instead of a string
        except (UnicodeDecodeError, json.JSONDecodeError):
            # If _content is not valid JSON, leave it as-is
            pass

    return pprint.pformat(response_dict)


def sync_call(config):
    """
    Executes a synchronous call to the specified function.
    """
    url = config.apihost+'/namespaces/_/actions/'+config.function+'?blocking=true&result=true&workers='+str(config.workers)
    response = requests.post(url, json=config.payload, headers={'Authorization': config.authorization})
    response.client_elapsed_time = response.elapsed.total_seconds() * 1000
    return response


def async_call(config):
    """
    Executes an asynchronous call to the specified function.
    It first posts the request to the function endpoint and then polls the activation to get the result.
    """
    url = config.apihost+'/namespaces/_/actions/'+config.function+'?blocking=false&result=true&workers='+str(config.workers)

    start_time = time.time()
    post_response = requests.post(url, json=config.payload, headers={'Authorization': config.authorization})
    activation_id = post_response.json()["activationId"]
    url = config.apihost+'/namespaces/_/activations/'+activation_id

    # Wait until the worker completes the job
    while True:
        get_response = requests.get(url, headers={'Authorization': config.authorization})
        if get_response.status_code == 200: # Activation completed
            break
        time.sleep(config.time_precision/1000)
        
    get_response.client_elapsed_time = (time.time() - start_time) * 1000

    return post_response, get_response


def run_single_benchmark(config):
    """
    Executes a single benchmark run based on the configuration.
    It handles both blocking and non-blocking calls.
    """
    if config.blocking:
        response = sync_call(config)
        #log.info(f"\n\nResponse: {response.__dict__}")
        log.info(f"\n\nResponse:\n{format_response_dict(response)}")
        return extract_metrics(response, config)
    else:
        post_response, get_response = async_call(config)
        log.info(f"\n\nPost response:\n{format_response_dict(post_response)}")
        log.info(f"\nGet response:\n{format_response_dict(get_response)}")
        return extract_metrics(get_response, config)


def run_benchmark_invocations(config, warmup=False):
    """
    Executes the specified number of invocations for a single benchmark run.
    Collects the metrics for each invocation.
    """
    if warmup:
        for _ in range(config.warmup_invocations):
            run_single_benchmark(config)
    else:
        metrics_list = []
        for _ in range(config.num_invocations):
            metrics = run_single_benchmark(config)
            metrics_list.append(metrics)
        return metrics_list


def benchmark(config):
    """
    Executes the benchmark process based on the configuration.
    Handles multiple runs and gathers statistics for each run.
    """
    all_metrics = []

    # Warm-up runs (ignored in results)
    log.info(f"\nStarting Warm-up")
    run_benchmark_invocations(config, warmup=True)

    # Main benchmark runs
    for run in range(config.num_runs):
        log.info(f"\nStarting run {run + 1}/{config.num_runs}")
        metrics_list = run_benchmark_invocations(config)
        all_metrics.extend(metrics_list)

    # Calculate statistics
    stats = benchmark_statistics(all_metrics)

    # Format and display results
    results = format_results(stats, config)
    print(results)

    # Write to file if output file is specified
    if config.output_file:
        write_results_to_file_csv(stats, config)
        

def main():
    config = Config()
    config.parse_arguments()
    config.print_config()
    benchmark(config)


if __name__ == '__main__':
    main()