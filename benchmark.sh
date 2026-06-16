#!/bin/bash

DURATION=${1:-2}
LOG_FILE=${2:-benchmark.log}
cargo build --release
./target/release/cefdetector &
APP_PID=$!
sleep 0.2
psrecord $APP_PID --duration $DURATION --log $LOG_FILE --interval 0.05 --include-children
kill -9 $APP_PID
wait $APP_PID 2>/dev/null
