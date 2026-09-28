DVRDesk Native for Windows
==========================

This folder contains:
  dvrdesk-native.exe          the DVRDesk app
  guide-history-service.exe   the optional Guide History service (below)

It isn't code-signed, so the first time you run it Windows may show
"Windows protected your PC". Click More info, then Run anyway.


Video playback (mpv)
--------------------

DVRDesk plays video with mpv's library, libmpv-2.dll, which is not
included in this download.

The first time you run DVRDesk, it offers to download it for you.
Click Download mpv, then Restart DVRDesk when it's done.

To install it by hand instead: download the newest
mpv-dev-x86_64-<date>-git-<id>.7z from
  https://github.com/shinchiro/mpv-winbuild-cmake/releases
open it with 7-Zip, and copy libmpv-2.dll into this folder, next to
dvrdesk-native.exe.

Note: 'winget install mpv' does not work for this. It installs mpv.exe
only, without libmpv-2.dll.


Guide History service (optional)
--------------------------------

Channels DVR's guide only shows what's on now and later. The Guide
History service keeps a record of the guide as it airs, so DVRDesk's
Live guide can show what was on earlier today and yesterday.

Run it on one PC that stays on, ideally the PC that runs Channels DVR.
Every DVRDesk on your network can then use it.

The easy way: run DVRDesk on that PC, open Settings > Programming
History, and click Install on This PC. It sets everything up and starts
the service with Windows. Windows asks for administrator permission
once.

To set it up by hand instead:

1. Sign in to the PC that will run the service, using the Windows
   account the service should run under.

2. Create a folder for it, for example C:\GuideHistory, and copy
   guide-history-service.exe into it.

3. Run it once to check it works. Open Command Prompt and type:

     cd C:\GuideHistory
     guide-history-service.exe --channels-dvr-url http://192.168.1.50:8089

   Replace http://192.168.1.50:8089 with the address of your Channels
   DVR server: the same address DVRDesk shows under Settings > Servers.
   It is NOT the address of this PC (unless Channels DVR runs on it).

   If Windows Firewall asks, allow it on Private networks, so other PCs
   can reach it.

   Open http://localhost:8790/health in a browser. If it shows
   "status":"ok", it's working. Press Ctrl+C in Command Prompt to stop
   it.

4. Start it automatically with Windows, using Task Scheduler:

   a. Open Task Scheduler and click Create Task... (not "Create Basic
      Task").
   b. General tab: name it DVRDesk Guide History, and select "Run
      whether user is logged on or not".
   c. Triggers tab: New..., set "Begin the task" to At startup, OK.
   d. Actions tab: New..., Start a program:
        Program/script:  C:\GuideHistory\guide-history-service.exe
        Add arguments:   --channels-dvr-url http://192.168.1.50:8089
        Start in:        C:\GuideHistory
      using your Channels DVR address from step 3. OK.
   e. Settings tab: untick "Stop the task if it runs longer than 3 days".
   f. Click OK and enter the account's Windows password.
   g. Right-click the new task and choose Run to start it now.

   Check http://localhost:8790/health again to confirm it's running.
   The history is saved in C:\GuideHistory\guide-history.json.

5. Point DVRDesk at it. If the service runs on the Channels DVR PC,
   DVRDesk finds it by itself when Settings > Programming History is
   left blank. Otherwise enter http://<that PC's address>:8790 there.
   (To find a PC's address, run ipconfig and look for IPv4 Address.)

The history fills in from the time the service starts, so yesterday's
guide appears after it has run for a day. The PC must stay on and awake
for it to keep recording.
