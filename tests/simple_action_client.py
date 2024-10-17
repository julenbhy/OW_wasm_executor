import json
import requests
import time

APIHOST='http://172.17.0.1:3233/api/v1'

headers = {'Authorization': 'Basic MjNiYzQ2YjEtNzFmNi00ZWQ1LThjNTQtODE2YWE0ZjhjNTAyOjEyM3pPM3haQ0xyTU42djJCS0sxZFhZRnBYbFBrY2NPRnFtMTJDZEFzTWdSVTRWck5aOWx5R1ZDR3VNREdJd1A='}

def sync_call(action_name: str, params: dict):
    url = APIHOST+'/namespaces/_/actions/'+action_name+'?blocking=true&result=true&workers=1'
    start_time = time.time()
    response = requests.post(url, json=params, headers=headers)
    elapsed_time = time.time() - start_time
    print('REQUEST:', response.request.__dict__)
    return response.text, elapsed_time


def async_call(action_name: str, params: dict):
    url = APIHOST+'/namespaces/_/actions/'+action_name+'?blocking=false&result=true&workers=1'

    start_time = time.time()
    response = requests.post(url, json=params, headers=headers)
    print('REQUEST:', response.request.__dict__)
    data = json.loads(response.text)
    activation_id = data["activationId"]
    url = APIHOST+'/namespaces/_/activations/'+activation_id

    # Wait until the worker completes the job
    while True:
        result = requests.get(url, headers=headers)
        if result.status_code == 200:
            break
        time.sleep(0.001)
        
    elapsed_time = time.time() - start_time
    result = json.loads(result.text)
    print('duration:', result['duration'], 'ms')
    return result['response']['result'], elapsed_time

def main():

    req_body = {"param1": 3, "param2": 1}
    print('req_body:', req_body)

    response ,elapsed_time = async_call('add', req_body)

    print('RESPONSE:', response)
    print('TIME TAKEN:', elapsed_time)


if __name__ == '__main__':
    main()

