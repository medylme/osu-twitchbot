# Linux Notes

**Memory reading requires ptrace permission.** Most distros ship with `ptrace_scope=1`, which only allows reading a *child* process's memory — so reading an independently-launched osu! fails with a permission error. Pick one of:

```bash
# option 1: allow ptrace between your own processes (until reboot)
sudo sysctl kernel.yama.ptrace_scope=0
# make it permanent:
echo "kernel.yama.ptrace_scope = 0" | sudo tee /etc/sysctl.d/10-ptrace.conf

# option 2: grant the capability to the binary only
sudo setcap cap_sys_ptrace+ep ./osu-twitchbot-linux-x86_64
```

**Token persistence requires a Secret Service daemon** (gnome-keyring or KWallet — present on any normal desktop). Without one, connecting still works but the token isn't remembered across restarts.

**The tray icon requires AppIndicator support.** On GNOME, install the [AppIndicator extension](https://extensions.gnome.org/extension/615/appindicator-support/); KDE and most other desktops work out of the box. If no tray is available, closing the window quits the app instead.
