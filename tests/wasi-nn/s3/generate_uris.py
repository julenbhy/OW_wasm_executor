import boto3

session = boto3.Session()
s3 = session.resource('s3')

my_bucket = s3.Bucket('rusty-bucket')

objs = list(my_bucket.objects.filter(Prefix='imagenet/'))

# Save in file
with open('../fixture/imagenet_keys.txt', 'w') as f:
    for obj in objs:
        f.write(obj.key + '\n')

with open('imagenet_uris.txt', 'w') as f:
    for obj in objs:
        s3_uri = f"s3://{my_bucket.name}/{obj.key}"
        f.write(s3_uri + '\n')