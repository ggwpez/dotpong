'''
Lists all metrics so that you can extract their IDs into the config.json

Needs the .env and config.json files that are mentioned in the README.
'''

import dotenv
import os
import subprocess
import requests
import time
import json

dotenv.load_dotenv()
with open('config.json') as f:
	cfg = json.load(f)

PAGE = cfg['page']
INSTATUS_KEY = os.getenv('INSTATUS_KEY')
METRICS_URL=f"https://api.instatus.com/v1/{PAGE}/metrics"

if INSTATUS_KEY is None:
	print('INSTATUS_KEY, PAGE, or SUBSTRATE_URI is not set')
	exit(1)

def get_metrics():
	req = requests.get(METRICS_URL, headers={'Authorization': f"Bearer {INSTATUS_KEY}"})
	if req.status_code != 200:
		print(f"Error: {req}")
		return []
	return req.json()

print(get_metrics())
