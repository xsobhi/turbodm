## Install

| System | Download |
|---|---|
| **Windows 10/11** | `TurboDM-…-windows-x64-setup.exe` (installs for all users and updates itself) or `…-portable.zip` |
| **Ubuntu 24.04+, Linux Mint 22+, Debian 13+** | `turbodm_…_amd64.deb` (or `arm64`) — also adds the apt repository, so updates come with your system updates |
| **Fedora 40+, openSUSE Tumbleweed** | `turbodm-…x86_64.rpm` (or `aarch64`) |

Or with apt:

```sh
sudo curl -fsSLo /usr/share/keyrings/turbodm-archive-keyring.gpg https://xsobhi.github.io/turbodm/turbodm-archive-keyring.gpg
sudo curl -fsSLo /etc/apt/sources.list.d/turbodm.sources https://xsobhi.github.io/turbodm/turbodm.sources
sudo apt update && sudo apt install turbodm
```

Then add the browser extension: see [Browser extension](https://github.com/xsobhi/turbodm#browser-extension).
