1. Run MyPod on your Mac
Open Terminal:
cd ~/Desktop/mypod/apps/desktop
npm run tauri dev


This launches the desktop version of MyPod.

2. Run MyPod on your iPhone SE
Keep your iPhone connected, unlocked, and available in Xcode.

From Terminal:
cd ~/Desktop/mypod/apps/desktop
npm run tauri ios dev


Select your physical iPhone SE if prompted.

3. Run the standalone production build
To create a production iOS build:
cd ~/Desktop/mypod/apps/desktop
npm run tauri ios build