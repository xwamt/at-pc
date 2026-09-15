#!/usr/bin/env python3
import os
import shutil
import hashlib
import zipfile

def sha256_file(filepath):
    h = hashlib.sha256()
    with open(filepath, 'rb') as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest()

def main():
    base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    target_dir = os.path.join(base_dir, 'target')
    dist_dir = os.path.join(base_dir, 'dist')
    os.makedirs(dist_dir, exist_ok=True)

    # 1. Clean runtime noise
    noise_files = ['at-pc-agent.log', 'at-pc-server.log', 'audit.jsonl', 'terminals_meta.json', '.DS_Store']
    for nf in noise_files:
        p = os.path.join(dist_dir, nf)
        if os.path.exists(p):
            os.remove(p)

    # 2. Source binaries
    mac_agent = os.path.join(target_dir, 'release', 'at-pc-agent')
    mac_server = os.path.join(target_dir, 'release', 'at-pc-server')
    win_agent = os.path.join(target_dir, 'x86_64-pc-windows-gnu', 'release', 'at-pc-agent.exe')
    win_server = os.path.join(target_dir, 'x86_64-pc-windows-gnu', 'release', 'at-pc-server.exe')

    # 3. Copy macOS
    shutil.copy2(mac_agent, os.path.join(dist_dir, 'at-pc-agent-macos'))
    shutil.copy2(mac_server, os.path.join(dist_dir, 'at-pc-server-macos'))
    shutil.copy2(mac_server, os.path.join(dist_dir, 'at-pc-macos'))
    for mac_bin in ['at-pc-agent-macos', 'at-pc-server-macos', 'at-pc-macos']:
        bin_path = os.path.join(dist_dir, mac_bin)
        os.chmod(bin_path, 0o755)
        # Clear quarantine attributes and sign with ad-hoc signature to satisfy macOS Gatekeeper / AMFI
        os.system(f'xattr -cr "{bin_path}" 2>/dev/null')
        os.system(f'codesign --force --deep --sign - "{bin_path}" 2>/dev/null')

    # 4. Copy Windows
    shutil.copy2(win_agent, os.path.join(dist_dir, 'at-pc-agent.exe'))
    shutil.copy2(win_server, os.path.join(dist_dir, 'at-pc-server.exe'))
    shutil.copy2(win_server, os.path.join(dist_dir, 'at-pc.exe'))

    # 5. Pack macOS ZIP
    mac_zip = os.path.join(dist_dir, 'at-pc-macos-arm64.zip')
    if os.path.exists(mac_zip):
        os.remove(mac_zip)
    with zipfile.ZipFile(mac_zip, 'w', zipfile.ZIP_DEFLATED) as z:
        for f in ['at-pc-server-macos', 'at-pc-agent-macos', 'at-pc-macos', 'agent_config.toml', 'server_config.example.toml', 'README.md']:
            z.write(os.path.join(dist_dir, f), arcname=f)

    # 6. Pack Windows ZIP
    win_zip = os.path.join(dist_dir, 'at-pc-windows-x86_64.zip')
    if os.path.exists(win_zip):
        os.remove(win_zip)
    with zipfile.ZipFile(win_zip, 'w', zipfile.ZIP_DEFLATED) as z:
        for f in ['at-pc-server.exe', 'at-pc-agent.exe', 'at-pc.exe', 'agent_config.toml', 'server_config.example.toml', 'README.md']:
            z.write(os.path.join(dist_dir, f), arcname=f)

    print('=== at-pc v1.0.0 Packaging Complete ===')
    for art in ['at-pc-server-macos', 'at-pc-agent-macos', 'at-pc-macos', 'at-pc-server.exe', 'at-pc-agent.exe', 'at-pc.exe', 'at-pc-macos-arm64.zip', 'at-pc-windows-x86_64.zip']:
        p = os.path.join(dist_dir, art)
        size_mb = os.path.getsize(p) / (1024 * 1024)
        cs = sha256_file(p)
        print(f'{art:<25} {size_mb:6.2f} MB  SHA256: {cs}')

if __name__ == '__main__':
    main()
