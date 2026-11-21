#!/bin/sh
# Renders the processor config from environment variables, so secrets live only in
# deploy/oracle/.env, then runs the processor in streaming (never-ending) mode.
set -eu

: "${POSTGRES_PASSWORD:?}" "${TRANSACTION_STREAM_API_KEY:?}" "${DYNEXUS_ADDRESSES:?}" "${STARTING_VERSION:?}"

# Values are written inside double-quoted YAML strings; reject characters that would
# break out of them instead of silently writing a broken config.
for name in POSTGRES_PASSWORD TRANSACTION_STREAM_API_KEY DYNEXUS_ADDRESSES STARTING_VERSION; do
  eval "value=\${$name}"
  case "$value" in
    *[\"\\\'\ ]* | *'$'*) echo "processor-entrypoint: $name contains quotes, backslashes, spaces or \$" >&2; exit 1 ;;
  esac
done
case "$STARTING_VERSION" in *[!0-9]*) echo "processor-entrypoint: STARTING_VERSION must be a number" >&2; exit 1 ;; esac

# The stream filter takes one address; all DYNEXUS packages share the deployer account.
filter_address=${DYNEXUS_ADDRESSES%%,*}

config=/tmp/processor.yaml
umask 077
cat > "$config" <<EOF
health_check_port: 8085
server_config:
  transaction_stream_config:
    indexer_grpc_data_service_address: "https://grpc.testnet.aptoslabs.com:443"
    auth_token: "$TRANSACTION_STREAM_API_KEY"
    request_name_header: "dynexus-processor"
    starting_version: $STARTING_VERSION
    transaction_filter:
      type: EventFilter
      struct_type:
        address: "$filter_address"
  postgres_config:
    connection_string: "postgresql://dynexus:$POSTGRES_PASSWORD@postgres:5432/dynexus"
EOF

exec dynexus-processor --config-path "$config"
