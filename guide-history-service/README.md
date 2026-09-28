# guide-history-service

Optional companion to DVRDesk Native. Channels DVR's own guide API is
forward-only — nothing exposes "what was on yesterday" once a program
airs. This service continuously polls your Channels DVR server's guide
feed and keeps a rolling window of what it's seen in one JSON file, then
serves it back out over a small HTTP API so DVRDesk's Live guide can show
the previous day and let you jump to what was recorded.

It's entirely optional and independent of any desktop client — run it
once, anywhere on your network (the DVR box itself, a NAS, a Raspberry Pi,
a spare Linux box), and point DVRDesk's Settings → Programming History at
it. If you never set this up, DVRDesk's guide works exactly as it does
without it.

## Running it

```sh
GHS_CHANNELS_DVR_URL=http://192.168.1.10:8089 cargo run --release
```

Each setting can be an environment variable or the matching command-line
flag (`--channels-dvr-url`, `--poll-secs`, `--retention-secs`,
`--fetch-window-secs`, `--listen`, `--data`); a flag wins when both are set.

| Variable               | Default              | Meaning                                                              |
|-------------------------|-----------------------|------------------------------------------------------------------------|
| `GHS_CHANNELS_DVR_URL`   | *(required)*          | Your Channels DVR server (not this service's host), e.g. `http://192.168.1.10:8089`. `GHS_SERVER_URL` is the older name and still works. |
| `GHS_POLL_SECS`          | `900` (15 min)         | How often to re-poll the guide feed                                    |
| `GHS_RETENTION_SECS`     | `172800` (48h)         | How long a captured slot is kept after it airs                         |
| `GHS_FETCH_WINDOW_SECS`  | `7200` (2h)            | How far forward each poll asks the DVR for                             |
| `GHS_LISTEN_ADDR`        | `0.0.0.0:8790`         | Where the HTTP API listens                                             |
| `GHS_DATA_PATH`          | `guide-history.json`   | Where the rolling history file is stored                               |

## HTTP API

- `GET /history?from=<unix>&to=<unix>` — every captured slot overlapping
  that time range, as JSON.
- `GET /health` — `{"status":"ok","programs_cached":N}`, for a simple
  reachability check.

## Installing it from DVRDesk (recommended)

DVRDesk Native ships with this service. On the PC that runs Channels DVR (or
another always-on PC), open DVRDesk's **Settings → Programming History** and
click **Install on This PC**. It sets everything from the active server and
starts the service at boot:

- **Linux:** a `systemd --user` unit, with `loginctl enable-linger` so it
  runs without anyone logged in. No sudo needed.
- **Windows:** a scheduled task that runs as SYSTEM at startup, plus a
  Windows Firewall rule opening the port to private networks. Windows asks
  for administrator permission once.

DVRDesk on other PCs checks for the service on the Channels DVR server's
host at port 8790 and uses it automatically when Programming History is
blank.

## Running it unattended yourself

Run the binary under whatever your host already uses for long-running
processes (a `systemd` unit, a Docker container, a NAS's task scheduler,
etc.), pointing `GHS_DATA_PATH` at a location that persists across
restarts. Use port 8790 on the Channels DVR host if you want DVRDesk
clients to find it automatically.
