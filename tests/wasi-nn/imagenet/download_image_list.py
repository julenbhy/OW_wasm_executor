# Download images in flicker_urls.txt
import requests
import json

urls = []
classes = []

with open("flicker_images.json", "r") as file:
    data = json.load(file)
    for key in data:
        urls.append(data[key])
        classes.append(key)

# Download images from urls
for url, class_name in zip(urls, classes):
    img_resp = requests.get(url)
    with open(f'images/{class_name}.jpg', 'wb') as f:
        f.write(img_resp.content)
