#!/bin/bash

set -e

while true
do
  cargo r -r --offline --frozen
  sleep 600
done
