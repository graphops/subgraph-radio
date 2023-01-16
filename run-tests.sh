#!/bin/bash

command_light="cargo test --package poi-radio --bin poi-radio -- tests --nocapture --ignored"
command_boot="cargo test --package poi-radio --bin poi-radio -- boot tests --nocapture --ignored"
command_display="cargo test --package poi-radio --bin poi-radio -- display tests --nocapture --ignored"

iterations=3

# Define an array to store the PIDs of the background processes
pids=()

start() {
  $command_boot &>/dev/null &
  pids+=($!)
  until lsof -i :60000 -sTCP:LISTEN >/dev/null; do
    echo "Waiting for the test boot node to start..."
    sleep 1
  done
  for i in $(seq 1 $iterations); do
    # Run the command in the background and redirect output to /dev/null
    $command_light &>/dev/null &
    # Store the PID of the background process
    pids+=($!)
  done
  $command_display
}

stop() {
  pid=$(lsof -t -i :60000)
  if [[ -n $pid ]]; then
    kill $pid
    while pgrep -f $pid &> /dev/null; do
      sleep 1
    done
  fi
}

# Run the start function immediately when the script is executed
start

# Use a trap to execute the stop function when the script is terminated
trap 'stop' INT TERM EXIT
