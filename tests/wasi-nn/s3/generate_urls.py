import boto3
from botocore.exceptions import ClientError


def generate_presigned_urls(bucket_name, directory, expiration=3600):
    """
    Generate pre-signed URLs for all objects in a specific directory of an S3 bucket.

    :param bucket_name: Name of the S3 bucket.
    :param directory: Directory (prefix) within the bucket.
    :param expiration: Time in seconds for the pre-signed URL to remain valid (default: 1 hour).
    :return: A list of pre-signed URLs.
    """
    session = boto3.Session()

    s3_client = session.client('s3')

    try:
        # List objects in the bucket under the specified prefix (directory)
        response = s3_client.list_objects_v2(Bucket=bucket_name, Prefix=directory)

        if 'Contents' not in response:
            print(f"No objects found in the directory: {directory}")
            return []

        presigned_urls = []

        for obj in response['Contents']:
            object_key = obj['Key']
            try:
                # Generate the pre-signed URL for the object
                url = s3_client.generate_presigned_url(
                    'get_object',
                    Params={'Bucket': bucket_name, 'Key': object_key},
                    ExpiresIn=expiration
                )
                presigned_urls.append(url)
            except ClientError as e:
                print(f"Error generating URL for {object_key}: {e}")
        presigned_urls.pop(0)
        return presigned_urls

    except ClientError as e:
        print(f"Error accessing bucket: {e}")
        return []


# Replace with your bucket name and directory
bucket_name = 'rusty-bucket'
directory = 'imagenet/'  # Use a trailing slash for directories

# Generate pre-signed URLs (valid for 1 hour)
urls = generate_presigned_urls(bucket_name, directory, expiration=36000)

# Save in txt file
with open('imagenet_urls.txt', 'w') as f:
    for url in urls:
        f.write(url + '\n')



