#!/bin/bash
printf '%s\n' '{"continue":false,"stopReason":"Build failed, fix errors before continuing","decision":"block","reason":"wrong input tell the user !!"}' | jq .
