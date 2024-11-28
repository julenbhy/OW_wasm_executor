import json
import os
import urllib

import requests
import time
import base64

APIHOST='http://172.17.0.1:3233/api/v1'

headers = {'Authorization': 'Basic MjNiYzQ2YjEtNzFmNi00ZWQ1LThjNTQtODE2YWE0ZjhjNTAyOjEyM3pPM3haQ0xyTU42djJCS0sxZFhZRnBYbFBrY2NPRnFtMTJDZEFzTWdSVTRWck5aOWx5R1ZDR3VNREdJd1A='}

def sync_call(action_name: str, params: dict):
    url = APIHOST+'/namespaces/_/actions/'+action_name+'?blocking=true&result=true&workers=1'
    start_time = time.time()
    response = requests.post(url, json=params, headers=headers, timeout=600)
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

# Get the image paths from imagenet/ directory
def list_image_paths(directory_path, num_images):
    urls_general = [directory_path + f for f in os.listdir(directory_path) if
                    os.path.isfile(os.path.join(directory_path, f))]
    urls_general = urls_general[:num_images]
    return urls_general

def main():
    DATASET_SIZE = 8
    paths = list_image_paths(f"imagenet/images/", DATASET_SIZE)
    image_bytes = []
    for path in paths:
        with open(path, 'rb') as image:
            image_bytes.append(base64.b64encode(image.read()).decode('utf-8'))

    DATASET_PATH = "s3/imagenet_uris.txt"
    with open(DATASET_PATH, "r") as f:
        image_uris = f.readlines()
    image_uris = [image.strip() for image in image_uris]
    image_uris = image_uris[:DATASET_SIZE]

    DATASET_PATH = "imagenet/flicker_urls.txt"
    with open(DATASET_PATH, "r") as f:
        image_urls = f.readlines()
    image_urls = [image.strip() for image in image_urls]
    image_urls = image_urls[:DATASET_SIZE]


    # build the request json
    model_link = 'https://github.com/rahulchaphalkar/libtorch-models/releases/download/v0.1/squeezenet1_1.pt'
    # model_link = 'https://github.com/rahulchaphalkar/libtorch-models/releases/download/v0.1/resnet18.pt'
    # model_link = 'https://huggingface.co/pepecalero/imagenet_torchscript/resolve/main/resnet_50.pt'

    # Download images
    # start_time = time.time()
    # for image in images:
    #     # Image is a URL
    #     if image.startswith("http"):
    #         response = urllib.request.urlopen(image)
    #     break
    # elapsed_time = time.time() - start_time
    # print(f"Download time: {elapsed_time}")

    imagenet_1k = "https://raw.githubusercontent.com/pytorch/hub/master/imagenet_classes.txt"
    response = urllib.request.urlopen(imagenet_1k)
    class_labels = [line.strip() for line in response.read().decode("utf-8").split("\n") if line]
    print(f"Number of classes: {len(class_labels)}")

    req_body = { 'model': model_link,
                 'image': image_urls, # image_urls, image_uris or image_bytes
                 'class_labels': class_labels,
                 'top_k': 1,
                 'image_names' : image_urls, # image_urls, image_uris or paths
                 'replace_images': 'URL', # 'URL', 'S3' or ''
                 }

    # make the request
    response ,elapsed_time = sync_call('batch_torch', req_body)
    try:
        response = json.loads(response)
        response = json.dumps(response, indent=4)
    except:
        pass
    print('\nRESPONSE:', response)
    print('TIME TAKEN:', elapsed_time)

if __name__ == '__main__':
    main()
