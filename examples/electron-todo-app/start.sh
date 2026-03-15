#!/bin/bash
set -e
cd /home/tester/electron-todo-app
npm install --production 2>&1
npx electron . --no-sandbox --disable-gpu --force-renderer-accessibility 2>&1
