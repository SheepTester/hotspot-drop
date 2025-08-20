<img src="./docs/screenshot.png" alt="File listing of a `target/` directory listing 3 files. The user has selected two files and is uploading them." height="300" style="float: right; clear: right;" />

# hotspot-drop

Share files insecurely without an internet connection between devices: one device hosts an HTTP server and a hotspot, and the other device connects to it.
I sometimes want to share files between devices, but Nearby/Quick Share doesn't work sometimes, and AirDrop doesn't work at all.

## Usage

Prerequisites: For Android, I recommend installing [Termux](https://termux.dev/).

1.  Download and run the appropriate binary from the [Releases page](https://github.com/SheepTester/hotspot-drop/releases).
    On Linux, you can use `uname -m` to determine your architecture.

    I'm only building binaries for Windows and Android because neither support AirDrop and both can host a hotspot.
    Note: WSL2 ports aren't accessible by other devices., so if you're on Windows, download the Windows executable; don't use WSL.

    You can use cURL to download the latest binary:

    - **Linux `x86_64`**:
      ```sh
      curl -L "https://github.com/SheepTester/hotspot-drop/releases/latest/download/hotspot-drop-x86_64-unknown-linux-gnu" > hotspot-drop
      ```
    - **Linux `aarch64`**:
      ```sh
      curl -L "https://github.com/SheepTester/hotspot-drop/releases/latest/download/hotspot-drop-aarch64-unknown-linux-gnu" > hotspot-drop
      ```
    - **Android `aarch64`**
      ```sh
      curl -L "https://github.com/SheepTester/hotspot-drop/releases/latest/download/hotspot-drop-aarch64-linux-android" > hotspot-drop
      ```

    Then, make it executable:

    ```sh
    chmod +x hotspot-drop
    ```

2.  Open a hotspot on one device, and connect to the hotspot on the other device.

3.  Run the binary, either from the command line or double-clicking it in File Explorer.
    On Windows, make sure to allow access to private networks.
    It will serve the files in the current directory.

    ```sh
    ./hotspot-drop
    ```

    The binary will print out a list of URLs in the console output. Hopefully, one of them works. One of the IPs should be accessible from the other device too.
    QR codes for the URLs are also displayed on directory pages; if you connecting to a Mac, you can have it scan via webcam with [my QR code scanner](https://sheeptester.github.io/qr/).
