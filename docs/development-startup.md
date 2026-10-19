# Starting MyPod for Development

This guide covers the backend, browser-based frontend, desktop app, and the iOS Simulator. Run each command in a separate Terminal window when the instructions say to keep it running.

## What needs to run

| What you want to open | FastAPI backend | Vite frontend | Tauri app |
| --- | --- | --- | --- |
| Frontend in a desktop browser | Yes, for library data | Yes | No |
| macOS desktop app | Yes, for library data | Started by Tauri | Yes |
| iPhone Simulator app | No; iOS uses its on-device catalog | Started by Tauri | Yes |

The iOS build currently supports the on-device import/catalog work. Native iOS audio playback is not implemented, so starting the app does not mean its playback controls can play imported tracks. The simulator also does not need the Mac's FastAPI server for its on-device catalog.

## One-time prerequisites

- Python 3 and the project virtual environment at `.venv`.
- Node.js/npm and the frontend dependencies in `apps/desktop/node_modules`.
- Rust/Cargo and the Tauri CLI installed through the project's npm dependencies.
- For iOS Simulator: Xcode, the iOS Simulator runtime, CocoaPods, and the initialized project under `apps/desktop/src-tauri/gen/apple/`.

If the Python environment has not been prepared yet, from the repository root run:

```sh
python3 -m venv .venv
source .venv/bin/activate
python -m pip install -e 'backend[dev]'
```

If frontend dependencies are not present, install from the lockfile:

```sh
cd apps/desktop
npm ci
```

These are setup commands, not commands to repeat every time you start MyPod.

## Start the FastAPI backend

Open Terminal window 1 and run:

```sh
cd /Users/divyaavanigadda/Desktop/mypod
source .venv/bin/activate
uvicorn app.main:app --app-dir backend --reload
```

Leave this terminal running. Check that the API responds:

```sh
curl http://127.0.0.1:8000/health
```

The expected response is `{"status":"ok"}`. The backend binds to localhost. Library scanning is manual; starting the backend does not scan a folder. Scans are rejected unless permitted roots have been configured with `MYPOD_SCAN_ROOTS`.

## Start the frontend in a browser

Open Terminal window 2:

```sh
cd /Users/divyaavanigadda/Desktop/mypod/apps/desktop
npm run dev
```

Open <http://127.0.0.1:1420>. Vite proxies `/api` requests to the backend at `http://127.0.0.1:8000`, so keep the backend running for library data. Stop Vite with **Ctrl+C**.

## Start the macOS desktop app

Keep the backend running in its own terminal. Open another terminal and run:

```sh
cd /Users/divyaavanigadda/Desktop/mypod/apps/desktop
npm run tauri -- dev
```

Tauri starts the Vite development server from the configured `beforeDevCommand`; do not also run `npm run dev` on port 1420. Close the app and press **Ctrl+C** in the Tauri terminal to stop the development session.

## Start the iPhone SE Simulator app

1. Open Xcode Simulator and boot the device named **MyPod iPhone SE**. If it is not listed, open Xcode's simulator/device manager and create or install an iPhone simulator that uses an installed iOS runtime.
2. Keep the backend stopped or running as desired; the iOS on-device library uses its native catalog and does not use the Mac FastAPI endpoint.
3. Open a terminal and run:

   ```sh
   cd /Users/divyaavanigadda/Desktop/mypod/apps/desktop
   npm run tauri -- ios dev --no-watch "MyPod iPhone SE"
   ```

   Tauri starts the Vite dev server using the project configuration and builds/launches the app in the selected simulator. Keep the terminal open while using the app. Press **Ctrl+C** there to end the development session.

If Tauri says it cannot find the simulator, check the exact device name and boot status:

```sh
xcrun simctl list devices available
```

If Xcode reports a signing error, copy the complete error before changing signing settings. Simulator builds generally do not require a personal device provisioning profile, while installation on a physical iPhone does require signing setup.

## Common issue: `npm run dev` says `package.json` is missing

The npm project is in `apps/desktop`, not at the repository root. Change into that folder before running npm commands:

```sh
cd /Users/divyaavanigadda/Desktop/mypod/apps/desktop
npm run dev
```

Activating `.venv` only selects the Python environment. It does not change the current directory or make npm commands available at the repository root.

## Which mode should I use?

- To work on the API and view the web UI: start the backend and `npm run dev`.
- To work on the macOS app: start the backend and `npm run tauri -- dev`.
- To work on the iPhone Simulator app: run `npm run tauri -- ios dev --no-watch "MyPod iPhone SE"`; the backend is not needed for the iOS on-device catalog.

The browser page at `127.0.0.1:1420` is the web frontend. It is not itself the iOS Simulator app.
