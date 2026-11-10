#!/bin/sh
# Keep a `jetcarp explore run` going until its deadline, restarting it (it resumes from the
# database) if it crashes.
#   scripts/explore-supervise.sh JETCARP_BINARY DB UNTIL_UNIX_TIME [extra `jetcarp explore run` args]
bin=$1; db=$2; until=$3; shift 3
while [ "$(date +%s)" -lt "$until" ] && [ ! -e "${db%.sqlite}.stop" ]; do
    echo "$(date -Is) starting" >> "${db%.sqlite}.supervise.log"
    "$bin" explore run --db "$db" --until "$until" "$@" >> "${db%.sqlite}.log" 2>&1
    code=$?
    echo "$(date -Is) exited with $code" >> "${db%.sqlite}.supervise.log"
    [ "$code" -eq 0 ] && [ ! -e "${db%.sqlite}.stop" ] && [ "$(date +%s)" -lt "$until" ] && sleep 5
    [ "$code" -ne 0 ] && sleep 15
done
