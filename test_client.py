import json
import requests

APIHOST='http://172.17.0.1:3233/api/v1'

headers = {
    'Authorization': 'Basic MjNiYzQ2YjEtNzFmNi00ZWQ1LThjNTQtODE2YWE0ZjhjNTAyOjEyM3pPM3haQ0xyTU42djJCS0sxZFhZRnBYbFBrY2NPRnFtMTJDZEFzTWdSVTRWck5aOWx5R1ZDR3VNREdJd1A='
}


#Get avaliable actions
#url = APIHOST+'/namespaces/guest/actions'
#response = requests.get(url, headers=headers)
#for action in response.json(): print(action['name'])


# Invoke add simple action
#url = APIHOST+'/namespaces/_/actions/add?blocking=true&result=true'
#req_body = {"param1": 2, "param2": 3}


# Invoke add burst action
url = APIHOST+'/namespaces/_/actions/add?blocking=true&result=true&workers=3'

worker1_payload = {"param1": 1, "param2": 1}
worker2_payload = {"param1": 2, "param2": 2}
worker3_payload = {"param1": 3, "param2": 3}

# create a list of workers
req_body = {
    "worker1": worker1_payload,
    "worker2": worker2_payload,
    "worker3": worker3_payload
}
print('req_body:', req_body)

response = requests.post(url, json=req_body, headers=headers)
print('Raw response', response.text)


data = json.loads(response.text)
unique_activation_ids = set(data["activationIds"])

for activation_id in unique_activation_ids:
    print(activation_id)
    url = APIHOST+'/namespaces/_/activations/'+activation_id+'/result'
    response = requests.get(url, headers=headers)
    print('Worker response:', response.text)

