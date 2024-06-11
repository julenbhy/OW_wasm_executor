import asyncio
import aiohttp
import time
import json

APIHOST='http://172.17.0.1:3233/api/v1'

headers = {'Authorization': 'Basic MjNiYzQ2YjEtNzFmNi00ZWQ1LThjNTQtODE2YWE0ZjhjNTAyOjEyM3pPM3haQ0xyTU42djJCS0sxZFhZRnBYbFBrY2NPRnFtMTJDZEFzTWdSVTRWck5aOWx5R1ZDR3VNREdJd1A='}

async def wait_for_activation(session, activation_id):
    url = f'{APIHOST}/namespaces/_/activations/{activation_id}'
    while True:
        async with session.get(url, headers=headers) as response:
            if response.status == 200:
                result_json = await response.json()
                if result_json.get('response', {}).get('status', '') == 'success':
                    return result_json
        await asyncio.sleep(0.01)

async def async_call(action_name: str, payload: str):
    url = f'{APIHOST}/namespaces/_/actions/{action_name}?blocking=false&result=true&workers=3'

    async with aiohttp.ClientSession() as session:
        # Perform POST requests for each payload and get activationId  
        start_time = time.time()
        async with session.post(url, json=payload, headers=headers) as response:
            response.raise_for_status()
            data = await response.json()
            # activationIds are ducplicated ¿?
            activation_ids = set(data["activationIds"]) 

        # Wait until all activations are completed in parallel
        tasks = [wait_for_activation(session, activation_id) for activation_id in activation_ids]
        results = await asyncio.gather(*tasks)

        elapsed_time = time.time() - start_time

    return results, elapsed_time


async def main():
    worker1_payload = {"param1": 1, "param2": 1}
    worker2_payload = {"param1": 2, "param2": 2}
    worker3_payload = {"param1": 3, "param2": 3}

    req_body = {
        "worker1": worker1_payload,
        "worker2": worker2_payload,
        "worker3": worker3_payload
    }
    print('req_body:', req_body)

    results, elapsed_time = await async_call('add', req_body)

    for idx, result in enumerate(results):
        print(f'Worker {idx+1} response:', result['response']['result'])
        print(f'Worker {idx+1} duration:', result['duration'], 'ms')

    print('Total time taken:', elapsed_time)


if __name__ == '__main__':
    asyncio.run(main())