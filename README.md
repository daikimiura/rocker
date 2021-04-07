# Rust + Docker = Rocker🤘
`Rocker` is a minimal docker implementation for educational purposes inspired by [gocker](https://github.com/shuveb/containers-the-hard-way). `Rocker` uses linux kernel features (namespace, cgroup, chroot etc.) to isolate container processes and limit available resourses.
<br />

<img width="940" alt="スクリーンショット 2021-04-08 1 28 27" src="https://user-images.githubusercontent.com/32577081/113901345-ba244980-9809-11eb-873e-c7146a4747a0.png">

<br />

# Usage
- Run a container
```
rocker run [OPTIONS] <image-name> <command>

OPTIONS:
        --cpus <cpus>
    -m, --mem <mem>
        --pids-limit <pids-limit>
```

- List running containers
```
rocker ps
```
- List images
```
rocker images
```
- Run a command in the existing container
```
rocker exec <container-id> <command>
```
- Delete an image
```
rocker rmi <image-hash>
``` 
<br />

# Requisites

- [libdbus](https://dbus.freedesktop.org/releases/dbus/) (1.6 or higher)
<br />

# Build
`$ cargo build`

The executable file is located at `./target/x86_64-unknown-linux-gnu/debug/rocker`

