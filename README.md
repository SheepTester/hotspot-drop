# hotspot-drop

Share files insecurely without an internet connection between devices: one device hosts an HTTP server and a hotspot, and the other device connects to it.
I sometimes want to share files between devices, but Nearby/Quick Share doesn't work sometimes, and AirDrop doesn't work at all.

## Usage

Prerequisites: For Android, I recommend installing [Termux](https://termux.dev/).

1.  Download and run the appropriate binary from the [Releases page](https://github.com/SheepTester/hotspot-drop/releases).
    On Linux, you can use `uname -m` to determine your architecture.

    I'm only building binaries for Windows and Android because neither support AirDrop and both can host a hotspot.

2.  Open a hotspot on one device, and connect to the hotspot on the other device.

3.  Run the binary, either from the command line or double-clicking it in File Explorer.
    On Windows, make sure to allow access to private networks.
    It will serve the files in the current directory.

    The binary will spit out a list of URLs; hopefully, one of them works. One of the IPs should be accessible from the other device too.
