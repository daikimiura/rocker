# Rust + Docker = Rocker🤘
`Rocker` is a minimal docker implementation for educational purposes inspired by [gocker](https://github.com/shuveb/containers-the-hard-way). `Rocker` uses linux kernel features (namespace, cgroup, chroot etc.) to isolate container processes and limit available resourses.
<br />

<img width="940" alt="スクリーンショット 2021-04-08 1 28 27" src="https://user-images.githubusercontent.com/32577081/113901345-ba244980-9809-11eb-873e-c7146a4747a0.png">


# Usage
- Run a container
```
rocker run [OPTIONS] <image-name> <command>

OPTIONS:
        --cpus <cpus>
    -m, --mem <mem>
        --pids-limit <pids-limit>
```
<img width="1051" alt="スクリーンショット 2021-04-08 1 34 30" src="https://user-images.githubusercontent.com/32577081/113902178-944b7480-980a-11eb-847d-65bcd8cffc77.png">

- List running containers
```
rocker ps
```
<img width="1051" alt="スクリーンショット 2021-04-08 1 35 00" src="https://user-images.githubusercontent.com/32577081/113902254-a5948100-980a-11eb-9fa8-0c6f14d3e9de.png">

- List images
```
rocker images
```
<img width="1051" alt="スクリーンショット 2021-04-08 1 36 21" src="https://user-images.githubusercontent.com/32577081/113902445-daa0d380-980a-11eb-84c5-2f70382cb618.png">

- Run a command in the existing container
```
rocker exec <container-id> <command>
```
<img width="1052" alt="スクリーンショット 2021-04-18 15 41 40" src="https://user-images.githubusercontent.com/32577081/115136714-9a3c2380-a05c-11eb-8198-6ef1fa78e644.png">


- Delete an image
```
rocker rmi <image-hash>
``` 


# Requisites

- [libdbus](https://dbus.freedesktop.org/releases/dbus/) (1.6 or higher)


# Build
`$ cargo build`

The executable file is located at `./target/x86_64-unknown-linux-gnu/debug/rocker`

