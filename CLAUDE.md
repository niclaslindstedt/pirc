# pirc — Developer Notes

## Log File Locations

Both `pircd` and `pirc` write system logs (not user chat messages) to `~/.pirc/logs/`.

| Process | Log directory |
|---------|--------------|
| pircd node-1 | `~/.pirc/logs/node-1/` |
| pircd node-2 | `~/.pirc/logs/node-2/` |
| pircd node-3 | `~/.pirc/logs/node-3/` |
| pirc client  | `~/.pirc/logs/client/` |

Log filenames are timestamped: `YYYY-MM-DD_HH-MM-SS.log`.

### Tailing logs

```sh
tail -f ~/.pirc/logs/node-1/*.log
tail -f ~/.pirc/logs/client/*.log
```

### Log format

Plain text with timestamps (no ANSI colour codes in file output).

## Docker dev cluster

`make dev` starts a 3-node `pircd` cluster via `docker-compose.dev.yml`. Each
container bind-mounts its log directory to the corresponding host path so logs
are accessible without `docker exec`:

```
~/.pirc/logs/node-1/   ← pircd-1 container
~/.pirc/logs/node-2/   ← pircd-2 container
~/.pirc/logs/node-3/   ← pircd-3 container
```
