# Application connections to Fono Server

Applications call Fono Server directly with a bearer key. They do not need
Fono Desktop, a local API relay, SSH credentials or the administrative key.
The API endpoint must already be reachable from the application's runtime.

## Create a connection in the panel

1. Open `/panel/` and sign in with an **admin** token.
2. Open **Приложения** and enter an application name, for example `my-app`.
   Names use 1–64 ASCII letters, digits, `_` or `-`, starting with a letter or digit.
3. Check **Адрес API для приложения**. This is the base origin, without `/v1`
   or an endpoint path. A configured `server.client_url` fills this field.
4. Click **Создать ключ**, then **Скопировать .env**. Place the two values in
   the consuming application's configuration:

   ```dotenv
   FONO_HOST=http://fono-server:17832
   FONO_TOKEN=<the newly issued application key>
   ```

These are suggested environment variable names. The consuming application must
explicitly support them or map them to its own settings. Creating a key does not
modify or configure another application.

The secret is returned once and remains only in the current panel tab. Copy it
before leaving the section, signing out or reloading. If clipboard access is
unavailable, select and copy the displayed snippet. Lost secrets cannot be
retrieved: revoke the old key and issue a new one. Revocation applies to the next
authenticated request; it does not undo already admitted recognition work.

## One-time server setup

Use the existing CLI to create the initial administrator:

```sh
fono-server --config /etc/fono/server.toml token create panel-admin --scope admin
```

For the Docker example, run the same command through the service:

```sh
docker compose -f compose.example.yml run --rm fono token create panel-admin --scope admin
```

Store this secret separately and use it only for administration. Ordinary
application keys have `recognize` scope; the panel cannot issue administrative
keys or revoke them. Existing recognition/admin CLI tokens continue to work.

The service account needs write permission to the private credential directory,
including creation of its lock and temporary files. The Docker example makes
only that bind mount writable and keeps its root filesystem read-only. An older
deployment with a read-only credentials mount must update that Fono mount before
panel issuance can work. The token file retains its owner and mode `0600` checks.

Optionally configure the consumer endpoint once:

```toml
[server]
client_url = "http://fono-server:17832"
```

The equivalent environment setting is `FONO_SERVER_CLIENT_URL`. This is a display
default, not a listener, proxy or firewall configuration. It must be an absolute
HTTP(S) origin without credentials, path, query or fragment; the hostname must
also be accepted by `server.allowed_hosts`. An omitted value leaves the panel
field empty rather than guessing from the browser's address.

Choose an address for the consuming runtime:

| Consumer                     | Address requirement                                                                        |
| ---------------------------- | ------------------------------------------------------------------------------------------ |
| Native process on the server | The host listener, commonly `http://127.0.0.1:17832`                                       |
| Separate Docker container    | A reachable private listener or Fono's service name on an explicitly shared Docker network |
| Another machine              | A reachable protected HTTP(S) endpoint with the corresponding Host allowlist entry         |

Inside a container, `127.0.0.1` points to that container. A host port published
only on loopback is not automatically reachable from another container. A
Windows SSH-forwarded panel address is not the server-side consumer address.
Copying `.env` does not establish this network route. No consumer service,
network or environment is changed by the Fono panel.

## HTTP contract

All three operations require an admin bearer token, obey the existing Host and
per-credential rate limits, and return `Cache-Control: no-store`:

| Operation                                   | Result                                                                                                         |
| ------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `GET /v1/clients`                           | Recognition-key names, scopes, creation dates and optional configured API origin; no secrets, salts or digests |
| `POST /v1/clients` with `{"name":"my-app"}` | `201` with name, scope, creation date and the one-time secret                                                  |
| `DELETE /v1/clients/my-app`                 | `204`; subsequent authorization with that key fails                                                            |

Requests cannot select `admin` scope. Duplicate names return `409`; invalid
names/JSON return `400`. Unknown consumers return `404`. Storage failures return
a technical error without a secret or filesystem path. Administrative writes
are never automatically replayed by the panel; after an uncertain create,
refresh the list and revoke/recreate the entry if its secret was not received.

Send `Authorization: Bearer <FONO_TOKEN>` to the ordinary recognition routes;
see [API documentation](server-api.md). Names isolate revocation and the existing
per-credential limits. This feature adds no transcript archive or per-application
usage statistics.

See the [acceptance record](qa/server-clients.md) for executed local checks and
the separate production verification steps.
