#!/bin/bash -eu

ROOT=$(pwd)
RUNTIME=${ROOT}/youki
cd integration_test/src/github.com/opencontainers/runtime-tools
GOPATH=${ROOT}/integration_test make runtimetest validation-executables
test_cases=(
  "default/default.t"
  "linux_cgroups_devices/linux_cgroups_devices.t"
  "linux_cgroups_hugetlb/linux_cgroups_hugetlb.t" 
  "linux_cgroups_pids/linux_cgroups_pids.t"
  "linux_cgroups_memory/linux_cgroups_memory.t"
  "linux_cgroups_network/linux_cgroups_network.t"
  "linux_cgroups_cpus/linux_cgroups_cpus.t"
  "linux_cgroups_relative_cpus/linux_cgroups_relative_cpus.t" 
  "linux_cgroups_relative_devices/linux_cgroups_relative_devices.t"
  "linux_cgroups_relative_hugetlb/linux_cgroups_relative_hugetlb.t" 
  "linux_cgroups_relative_memory/linux_cgroups_relative_memory.t"
  "linux_cgroups_relative_network/linux_cgroups_relative_network.t" 
  "linux_cgroups_relative_pids/linux_cgroups_relative_pids.t"
  "create/create.t"
  "kill/kill.t"
  "delete/delete.t"
  "state/state.t"
  "linux_sysctl/linux_sysctl.t"
  "hooks/hooks.t"
  "prestart/prestart.t"
  "poststart/poststart.t"
  "prestart_fail/prestart_fail.t"
  "poststart_fail/poststart_fail.t"
  "poststop/poststop.t"
  "hooks_stdin/hooks_stdin.t"
)

# Record the tests that runc also fails to pass below, maybe we will fix this by origin integration test, issue: https://github.com/containers/youki/issues/56
# no_paas_test_case=(
#   "start/start.t" 
# )
for case in "${test_cases[@]}"; do
  echo "Running $case"
  if [ 0 -ne $(sudo RUST_BACKTRACE=1 YOUKI_LOG_LEVEL=debug RUNTIME=${RUNTIME} ${ROOT}/integration_test/src/github.com/opencontainers/runtime-tools/validation/$case | grep "not ok" | wc -l) ]; then
      exit 1
  fi
  sleep 1
done
