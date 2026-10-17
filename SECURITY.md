# Reporting a vulnerability

Send a private report to the project maintainer at
[gorbach.d@outlook.com](mailto:gorbach.d@outlook.com), the contact used on the
Fono website. Use a subject such as `Fono security report` and include:

- the affected Fono version and Windows version;
- the component and a minimal reproduction;
- the impact and any relevant sanitized technical details.

Do not open a public issue with an exploit, credentials, personal recordings,
dictation contents or a signing key. Do not include third-party field contents.
If private contact is unavailable, a public issue may ask for a private contact
without disclosing the vulnerability.

Please distinguish source, a published installer, an installed version and a
custom build. Include hashes or a release link when the package matters. A
report can describe an older release; identify its exact version rather than
assuming all versions share the defect. This document promises no response or
fix deadline.

The normal update and optional backend trust paths must verify their pinned
channel, signature, signed version and content hashes. Do not work around a
failed verification by disabling it. Product privacy is described in the
[user guide](docs/user-guide.md); dependency evidence is tracked in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
