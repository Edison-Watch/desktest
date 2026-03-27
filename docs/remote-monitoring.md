# Remote Monitoring

When running desktest on a remote machine (e.g. a dedicated test server), you can access the live monitoring dashboard and VNC from your local workstation.

## Option A: Bind to all interfaces (recommended for Tailscale / private networks)

Use `--monitor-bind-addr 0.0.0.0` to make the dashboard reachable from other machines:

```bash
desktest run task.json --monitor --monitor-bind-addr 0.0.0.0
```

Then open `http://<remote-host>:7860` in your browser.

For VNC, set `vnc_bind_addr` and `vnc_port` in your config JSON:

```json
{
  "vnc_bind_addr": "0.0.0.0",
  "vnc_port": 5900
}
```

> **Note:** Neither the dashboard nor VNC have authentication. Only use `0.0.0.0` on trusted networks (e.g. Tailscale, private LAN).

## Option B: SSH port forwarding

If you access the remote machine via SSH, forward the ports through the tunnel:

```bash
ssh -L 7860:127.0.0.1:7860 -L 5900:127.0.0.1:5900 user@remote-host
```

Then on the remote machine, run desktest with the default bind address (no extra flags needed):

```bash
desktest run task.json --monitor
```

On your local machine, open `http://localhost:7860` for the dashboard and connect a VNC viewer to `localhost:5900`.

This approach keeps both endpoints bound to localhost and requires no config changes.

## Combining both

You can forward just VNC over SSH while exposing the dashboard directly, or vice versa. Mix and match based on your network setup.
