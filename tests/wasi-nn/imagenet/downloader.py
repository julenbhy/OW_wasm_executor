import os
from enum import unique

import requests
import json
import time
import threading
from urllib.parse import urlparse
from requests.exceptions import ConnectionError, ReadTimeout, TooManyRedirects, MissingSchema, InvalidURL
import numpy as np
from concurrent.futures import ThreadPoolExecutor

class ImageNetScraper:
    def __init__(self, total_images, save_images=True, scrape_only_flickr=True):
        self.total_images = total_images
        self.save_images = save_images
        self.scrape_only_flickr = scrape_only_flickr

        class_info_json_filepath = 'imagenet_class_info.json'
        with open(class_info_json_filepath) as class_info_json_f:
            self.class_info_dict = json.load(class_info_json_f)

        self.classes_to_scrape = []
        self.downloaded_images = {}

    def imagenet_api_wnid_to_urls(self, wnid):
        return f'http://www.image-net.org/api/imagenet.synset.geturls?wnid={wnid}'

    def scrape_images(self):
        self.classes_to_scrape = []
        for key, val in self.class_info_dict.items():
            if (self.scrape_only_flickr and int(val['flickr_img_url_count']) > 1):
                self.classes_to_scrape.append(key)

        print("Picked the following classes:")
        print([self.class_info_dict[class_wnid]['class_name'] for class_wnid in self.classes_to_scrape])

        for class_wnid in self.classes_to_scrape:
            class_name = self.class_info_dict[class_wnid]["class_name"]
            print(f'Scraping images for class \"{class_name}\"')
            url_urls = self.imagenet_api_wnid_to_urls(class_wnid)
            resp = requests.get(url_urls)
            urls = [url.decode('utf-8') for url in resp.content.splitlines()]
            for url in urls:
                time.sleep(0.5)
                succeded = self.get_image(url, class_name)
                if succeded:
                    break
            # with ThreadPoolExecutor(max_workers=1) as executor:  # Adjusted max_workers for efficiency
            #     executor.map(self.get_image, urls, [class_name] * len(urls))  # Pass class_wnid to ensure class uniqueness
            if len(self.downloaded_images) >= self.total_images:
                break

        if self.save_images:
            if not os.path.exists('images'):
                os.makedirs('images')
            with ThreadPoolExecutor(max_workers=1) as executor:
                executor.map(self.save_image, self.downloaded_images.values(), self.downloaded_images.keys())

        return self.downloaded_images

    def save_image(self, img_url, class_name):
        img_resp = requests.get(img_url)
        with open(f'images/{class_name}.jpg', 'wb') as f:
            f.write(img_resp.content)

    def get_image(self, img_url, class_name):
        if class_name in self.downloaded_images:
            return

        if self.scrape_only_flickr and 'flickr' not in img_url:
            return

        try:
            img_resp = requests.get(img_url, timeout=1)
        except (ConnectionError, ReadTimeout, TooManyRedirects, MissingSchema, InvalidURL):
            print(f"Error for url {img_url}")
            return

        if 'content-type' not in img_resp.headers or 'image' not in img_resp.headers['content-type'] or len(img_resp.content) < 1000:
            print(f"Not an image or invalid content for url {img_url}")
            return

        img_name = os.path.basename(urlparse(img_url).path).split("?")[0]
        if not img_name or "gif" in img_name:
            return

        if len(self.downloaded_images) >= self.total_images:
            return

        # Mark that we have downloaded an image for this class
        self.downloaded_images[class_name] = img_url
        return True

scraper = ImageNetScraper(total_images=128)
images = scraper.scrape_images()
# Save in json
with open('imagenet_images.json', 'w') as f:
    json.dump(images, f, indent=4)

with open ('imagenet_images.txt', 'w') as f:
    for key, value in images.items():
        f.write(f"{value}\n")