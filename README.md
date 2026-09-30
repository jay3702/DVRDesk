# DVRDesk

DVRDesk is a desktop app for Windows and Linux that lets you browse and watch everything on your [Channels DVR](https://getchannels.com) server: recordings, TV shows, movies, your video library and live TV.

It's built for everyday viewing at a desk with a mouse and keyboard, as an easier alternative to the Channels DVR web admin page. It isn't a 10-foot, remote-control TV interface.

> **This is DVRDesk Native (2.x)**, a rewrite of the original DVRDesk with a much more reliable player. The original app is now [DVRDesk 1.x (legacy)](docs/LEGACY-1x.md).

## Download

Get the newest release from the **[Releases page](https://github.com/jay3702/DVRDesk/releases/latest)**. Pick the file for your computer:

| Your computer | Download |
|---|---|
| Windows (most PCs) | `DVRDesk-Native-Windows-x86_64.zip` |
| Windows on ARM (Snapdragon, Surface Pro X) | `DVRDesk-Native-Windows-aarch64.zip` |
| Ubuntu, Debian, Mint | `dvrdesk-native_<version>_amd64.deb` (or `_arm64.deb`) |
| Other Linux distributions | `DVRDesk-Native-Linux-x86_64.AppImage` (or `-aarch64`) |

Not sure if your Windows PC is ARM? Open **Settings → System → About** and look at **System type**.

## Install

### Windows

1. Unzip the download into a folder of its own, such as `C:\DVRDesk`.
2. Run `dvrdesk-native.exe`. DVRDesk isn't code-signed, so Windows may show "Windows protected your PC". Click **More info**, then **Run anyway**.
3. The first time it runs, DVRDesk asks to download mpv, which it uses to play video. Click **Download mpv**, then **Restart DVRDesk** when it's done.

The zip includes `README-Windows.txt` with more detail, including how to install mpv by hand.

### Linux

DVRDesk plays video with your system's libmpv.

- **.deb:** install it with `sudo apt install ./dvrdesk-native_<version>_amd64.deb`. This installs libmpv too, and adds DVRDesk to your app menu.
- **AppImage:** install libmpv first, then make the AppImage executable and run it:

  | Distribution | Install libmpv |
  |---|---|
  | Debian / Ubuntu | `sudo apt install libmpv2` |
  | Fedora | `sudo dnf install mpv-libs` |
  | Arch | `sudo pacman -S mpv` |

  ```sh
  chmod +x DVRDesk-Native-Linux-x86_64.AppImage
  ./DVRDesk-Native-Linux-x86_64.AppImage
  ```

## Getting started

1. Open **Settings → Servers** and add your Channels DVR server, for example `http://192.168.1.50:8089`. You can add several servers and switch between them. If you use Tailscale, you can also give each server a Tailscale address for when you're away from home.
2. Pick a section from the sidebar and start watching.

## Features

- **Recent recordings, TV shows, movies, videos and collections**, with search across all of them.
- **Live TV**, with a program guide grid and genre filters (Movies, Sports, Drama, News, Kids).
- **Resume where you left off**, with watched status synced to your DVR.
- **Captions:** broadcast captions and Channels DVR's Py-Captions, with adjustable size, color and background.
- **Commercial skip** for recordings with detected commercials.
- **Playback speed** from 0.5x to 2x for recordings and videos.
- **Downloads** to watch recordings offline.
- **Keyboard control**, with keys you can change in Settings. The defaults:

  | Action | Keys |
  |---|---|
  | Play / pause | Space, K |
  | Skip back 10 seconds | ←, J |
  | Skip forward 30 seconds | →, L |
  | Jump back / forward further | Shift+←, Shift+→ |
  | Close the player | Esc |

- **Light, dark or system theme.**

## Guide history (optional)

Channels DVR's guide only shows what's on now and later. The optional **Guide History service** keeps a record of the guide as it airs, so DVRDesk's Live guide can also show what was on earlier today and yesterday.

Run it on one computer that stays on, ideally the one running Channels DVR. Every DVRDesk on your network can then use it. The easiest way to set it up is to run DVRDesk on that computer, open **Settings → Programming History**, and click **Install on This PC**. DVRDesk finds the service automatically when it runs on the Channels DVR computer.

To set it up by hand, or on a NAS or Raspberry Pi, see the [guide-history-service README](guide-history-service/README.md).

## Coming from DVRDesk 1.x

Your servers and preferences can come with you:

- **Automatically:** open DVRDesk Native, go to **Settings**, and click **Import Automatically**. It reads 1.x's settings from the same computer, and 1.x can stay installed.
- **From another computer:** in DVRDesk 1.x (v1.15.0 or newer), open **Settings → Migrate to DVRDesk Native**, click **Copy Settings for Migration**, and paste the result into DVRDesk Native's Settings.

## Building from source

DVRDesk Native is written in Rust (egui for the interface, libmpv for playback) and lives in [`native/`](native/).

```sh
cargo build --release --manifest-path native/Cargo.toml
cargo build --release --manifest-path guide-history-service/Cargo.toml
```

You need a stable Rust toolchain, and on Windows the MSVC build tools. No libmpv headers are needed to build: the app loads libmpv when it starts, so it only needs libmpv installed to run. Release builds come from [`.github/workflows/native-release.yml`](.github/workflows/native-release.yml), triggered by `native-v*` tags.

The legacy 1.x app (Tauri, React and TypeScript) is still in this repository, in `src/` and `src-tauri/`. See [DVRDesk 1.x (legacy)](docs/LEGACY-1x.md) for how to build it.

## Acknowledgments

DVRDesk Native owes a real debt to [Clicker](https://github.com/mackid1993/Clicker), mackid1993's native Channels DVR client. Its open source was a working reference for embedding libmpv in egui, direct-file playback, and the design of downloads and theming. Thank you, mackid1993.

## License

[MIT](LICENSE)
