# Fono Server TLS and certificate renewal

The Ubuntu server supports native rustls HTTPS with TLS 1.2/1.3 and ALPN for
HTTP/1.1 and HTTP/2. Plain HTTP remains the default when `[tls]` is absent.
Supplying TLS material selects HTTPS on the configured port; there is no
plaintext fallback or automatic redirect.

```toml
[tls]
cert = "/etc/fono/tls/cert.pem"
key = "/etc/fono/tls/key.pem"
```

`cert.pem` contains the leaf certificate followed by any intermediates. Keys
must be unencrypted PEM and match the leaf. Startup rejects invalid PEM,
expired/not-yet-valid certificates, mismatched keys and unsafe key permissions
before binding. Files are bounded to 1 MiB and must be regular files. The open
key descriptor must belong to root or the service account, permit no access to
others and no group writes or execute bits; 0600, 0400 and trusted read-only
group permissions such as 0640 are supported. Symlinks may point into the
operator's certificate layout; the opened target is checked.

Non-loopback use still requires `trusted_private_network=true` and an explicit
`allowed_hosts` list under the [network policy](server-security.md). TLS does
not remove bearer authentication or authorize exposing the service publicly.
Add the certificate DNS name to the Host allowlist.

## Private CA example

Create a temporary private CA and a server certificate in a deployment folder
outside the Git checkout on the operator's machine. Substitute the DNS name and
IP that clients will actually use:

```sh
install -d -m 700 private-ca tls
openssl req -x509 -newkey rsa:3072 -sha256 -nodes -days 30 \
  -subj '/CN=Fono private CA' -addext 'basicConstraints=critical,CA:TRUE' \
  -addext 'keyUsage=critical,keyCertSign,cRLSign' \
  -keyout private-ca/ca.key -out tls/ca.pem
openssl req -new -newkey rsa:3072 -sha256 -nodes \
  -subj '/CN=fono.internal' -keyout tls/key.pem -out private-ca/server.csr
cat > private-ca/server.ext <<'EXT'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=DNS:fono.internal,IP:192.168.1.8
EXT
openssl x509 -req -in private-ca/server.csr -CA tls/ca.pem \
  -CAkey private-ca/ca.key -CAcreateserial -days 30 -sha256 \
  -extfile private-ca/server.ext -out tls/cert.pem
chmod 600 private-ca/ca.key tls/key.pem
```

Keep the CA private key off the service volume. Deliver only `cert.pem`,
`key.pem` and the public `ca.pem` to the operator-controlled TLS directory.
Give the service UID 10001 traversal/read access and retain mode 0600 for its
key. For example, on the deployment host:

```sh
sudo chown -R 10001:10001 tls
sudo chmod 700 tls
sudo chmod 600 tls/key.pem
```

Clients explicitly trust the public CA for this connection:

```sh
curl --fail --cacert tls/ca.pem \
  --resolve fono.internal:17832:192.168.1.8 \
  https://fono.internal:17832/healthz
```

Use the client's equivalent CA-file or certificate trust setting for API
requests. Do not disable certificate verification. The server's file checks
cannot establish a client's trust or make a hostname mismatch acceptable.

## Docker Compose

Use the [TLS override](../packaging/docker/compose.tls.example.yml) with the
base Compose file. Mount the **whole directory** read-only so replacement via
rename becomes visible. A file bind can keep the old inode after renewal.

```sh
export FONO_TLS_SERVER_NAME=fono.internal
export FONO_TLS_CA_FILE=/etc/fono/tls/ca.pem
docker compose -f compose.example.yml -f compose.tls.example.yml config --quiet
docker compose -f compose.example.yml -f compose.tls.example.yml run --rm fono doctor
docker compose -f compose.example.yml -f compose.tls.example.yml up -d --no-build
```

The override switches the healthcheck to HTTPS, verifies the CA and certificate
name, and resolves that name to loopback inside the container. For certificates
issued by a public CA, leave `FONO_TLS_CA_FILE` unset to use the image's system
CA bundle. The base image's HTTP healthcheck remains suitable for HTTP mode.

## Renewal and SIGHUP

Stage a complete new cert/key pair with the required permissions, replace both
files, and only then request reload:

```sh
docker compose -f compose.example.yml -f compose.tls.example.yml kill -s HUP fono
# A directly managed process can instead receive: kill -HUP <server-pid>
```

Reload parses, checks dates/permissions and validates the matching pair on a
blocking worker. Only a successful validation publishes a new rustls config.
New connections then use it; existing TLS connections and active requests keep
their previous config and continue. Failed renewal keeps the last validated
config and emits `tls_reload_rejected`; success emits `tls_reloaded`. It never
falls back to HTTP, resets request quotas or rewrites files.

SIGHUP reloads TLS material only. Restart explicitly for changes to the bind,
token-bucket settings, model placement or other configuration. Renew before
expiry; the server does not issue certificates or extend certificate validity.
Connection resumption state is not shared between certificate generations.

TLS transport admits at most 128 connections, bounds handshakes and HTTP/1
header reception to 10 seconds, and limits HTTP/2 streams to 32 per connection.
Shutdown stops reload admission, cancels unfinished handshakes and gracefully
drains connections within the server's existing shutdown deadline. An already
started certificate read retains its activity lease until the actual work ends.

## Tailscale certificates

Follow [Tailscale's HTTPS setup](https://tailscale.com/docs/how-to/set-up-https-certificates)
and issue a certificate for this machine's full tailnet DNS name with
`tailscale cert`. Supply explicit `--cert-file` and `--key-file` destinations,
then install the resulting files in the protected TLS directory. Use that DNS
name in clients, `allowed_hosts` and the Compose healthcheck.
Tailscale's HTTPS enablement includes consent to publishing this DNS name in
Certificate Transparency; follow its documented enablement step before issuance.

CLI-generated certificate files need an operator-managed renewal process.
After successful renewal, stage the pair and send SIGHUP as above. Issuance,
renewal scheduling and trust-store changes are separate deployment actions;
Fono does not make them automatically. This implementation pins rustls 0.23.46,
including the [upstream security fixes](https://github.com/rustls/rustls/releases/tag/v%2F0.23.46).
The [Ubuntu/CUDA acceptance record](qa/server-tls.md) lists the tested artifact,
trust and reload scenarios, quota checks and remaining deployment steps.
