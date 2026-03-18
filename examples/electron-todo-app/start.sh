#!/bin/bash
set -e
cd /home/tester/electron-todo-app
npx electron . --no-sandbox --disable-gpu --force-renderer-accessibility 2>&1
