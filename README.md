# orda-server

Self-hosted communication server for [Orda](https://joinorda.com). Handles real-time messaging, voice/video signaling, channels, members, and server management.

Part of a three-tier architecture:

- **orda-server** (this repo) runs on your infrastructure, stores messages in an encrypted database, and manages channels and members
- **The central API** handles accounts, authentication, licensing, TLS certificates, and DNS
- **The desktop app** connects to both

The central API never sees your messages. It only knows that an account exists and which servers it belongs to.

## Deploy

The recommended way to deploy is with the [installer](https://github.com/orda-oss/installer), which handles TLS, DNS, firewall, Docker, and all configuration automatically.

## Contributing

This repo is source-available for transparency and security auditing. We do not accept external pull requests at this time. If you find a bug or security issue, please open an issue or email [hello@joinorda.com](mailto:hello@joinorda.com).

## Support

Orda is built and maintained by a single person. If you find it useful, a donation goes a long way toward keeping the project alive. A donation channel is being set up and will be linked here soon.

If you or your organization are interested in sponsoring or making a larger contribution before the donation channel is live, reach out at [hello@joinorda.com](mailto:hello@joinorda.com).

In the meantime, the best way to support the project is to use it, report bugs, and spread the word. If you have legal expertise, the [Privacy Policy](https://orda.chat/privacy) and [Terms of Service](https://orda.chat/terms) are open for review on [GitHub](https://github.com/orda-oss/web).

## License

[GNU Affero General Public License v3.0](LICENSE)
