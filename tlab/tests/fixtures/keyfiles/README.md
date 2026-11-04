# Key file fixtures

These are public, disposable test keys. Never use them in an application.

Generated with OpenSSL 3.6.3:

```sh
openssl genpkey -algorithm ED25519 -out ed25519-private.pem
openssl pkey -in ed25519-private.pem -pubout -out ed25519-public.pem
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -noenc -keyout tls-private.pem -out tls-cert.pem -subj /CN=localhost -addext subjectAltName=DNS:localhost,IP:127.0.0.1 -days 30
openssl req -x509 -newkey rsa:2048 -noenc -keyout rsa-private.pem -out rsa-cert.pem -subj /CN=localhost -days 30
```

The Ed25519 private key is PKCS#8 v1; generation tests exercise rcgen's v2.
Fixture loading tests check formats and matching keys, not certificate expiry
or trust. Those checks belong to TLS consumers.
