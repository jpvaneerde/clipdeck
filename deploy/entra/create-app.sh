#!/usr/bin/env bash
# Creates the Clipdeck app registration in your Entra ID tenant with the Admin
# and User app roles, and requires assignment so only people you assign can
# sign in. Needs the Azure CLI, signed in with `az login` as someone who may
# create app registrations. Works in Git Bash or WSL.
#
#   ./deploy/entra/create-app.sh https://clipdeck.jplay.cloud
set -euo pipefail
URL="${1:-https://clipdeck.jplay.cloud}"
URL="${URL%/}"
HERE="$(cd "$(dirname "$0")" && pwd)"
ROLES="$HERE/app-roles.json"
# Git Bash: az is a Windows program and needs a Windows path for the @file argument.
if command -v cygpath >/dev/null 2>&1; then ROLES="$(cygpath -w "$ROLES")"; fi

APP_ID=$(az ad app create \
  --display-name "Clipdeck" \
  --sign-in-audience AzureADMyOrg \
  --web-redirect-uris "$URL/auth/callback" "$URL/auth/signed-out" \
  --app-roles "@$ROLES" \
  --query appId -o tsv)
echo "App registration created: $APP_ID"

# The enterprise application (service principal), with assignment required:
# people without an assigned role are stopped by Entra before reaching Clipdeck.
az ad sp create --id "$APP_ID" >/dev/null
az ad sp update --id "$APP_ID" --set appRoleAssignmentRequired=true

SECRET=$(az ad app credential reset --id "$APP_ID" --display-name "clipdeck-server" --years 2 --query password -o tsv)
TENANT=$(az account show --query tenantId -o tsv)

cat <<MSG

Done. Create the Kubernetes secret (the client secret is shown only now):

  kubectl create namespace clipdeck --dry-run=client -o yaml | kubectl apply -f -
  kubectl -n clipdeck create secret generic clipdeck-oidc \\
    --from-literal=OIDC_TENANT_ID=$TENANT \\
    --from-literal=OIDC_CLIENT_ID=$APP_ID \\
    --from-literal=OIDC_CLIENT_SECRET='$SECRET'

Then assign people or groups to the Admin and User roles:
  Entra admin center › Enterprise applications › Clipdeck › Users and groups › Add user/group
The client secret expires in 2 years.
MSG
