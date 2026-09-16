import { restPaths } from "../api/index.js";
import { openRemoteDesktop } from "../desktop/session.js";
import { byId, qsAll } from "../dom.js";
import { authFetch } from "../http.js";
import { closeModal, showModal } from "../ui.js";
import { escapeHtml, filterTerminals, parseTagList, processRowHtml, terminalCardHtml } from "./index.js";
import { terminalsState } from "./state.js";

export function setFilterTab(tab, btn) {
  terminalsState.currentFilterTab = tab;
  qsAll(".filter-tab").forEach((item) => {
    item.classList.remove("active");
  });
  if (btn) btn.classList.add("active");
  renderCards();
}

export function renderCards() {
  const search = byId("searchInput").value.toLowerCase().trim();
  const grid = byId("terminalGrid");
  const filtered = filterTerminals(terminalsState.terminalsData, {
    tab: terminalsState.currentFilterTab,
    search,
  });

  const onlineTotal = terminalsState.terminalsData.filter(
    (t) => t.status === "Online",
  ).length;
  byId("onlineCount").innerText = String(onlineTotal);
  byId("totalCount").innerText = String(
    terminalsState.terminalsData.length,
  );

  if (filtered.length === 0) {
    grid.innerHTML = `
          <div class="empty-state">
            <h3>暂无匹配的终端设备</h3>
            <p style="margin-top: 8px; font-size: 13px;">请在被协助电脑上启动 <code>at-pc-agent</code>，启动后将自动实时同步在此处</p>
          </div>
        `;
    return;
  }

  grid.innerHTML = filtered.map((t) => terminalCardHtml(t)).join("");
}

export async function fetchTerminals() {
  try {
    const res = await authFetch(restPaths.terminals());
    if (res.ok) {
      terminalsState.terminalsData = await res.json();
      renderCards();
    }
    checkRunningCalls();
  } catch (err) {
    console.error("Failed to fetch terminals:", err);
  }
}

export async function checkRunningCalls() {
  try {
    const res = await authFetch(restPaths.calls());
    if (res.ok) {
      const data = await res.json();
      terminalsState.runningCallsData = data.calls || [];
      const count = terminalsState.runningCallsData.length;
      const btn = byId("runningCallsBtn");
      const countSpan = byId("runningCallsCount");
      if (btn && countSpan) {
        countSpan.innerText = String(count);
        btn.style.display = count > 0 ? "inline-flex" : "none";
      }
    }
  } catch (err) {
    console.debug("Failed to check running calls:", err);
  }
}

export async function showRunningCallsModal() {
  await checkRunningCalls();
  let bodyHtml = "";
  if (!terminalsState.runningCallsData || terminalsState.runningCallsData.length === 0) {
    bodyHtml = `
          <div style="text-align: center; padding: 28px 0; color: var(--text-muted);">
            <div style="font-size: 32px; margin-bottom: 10px;">☕</div>
            <div style="font-size: 14px; font-weight: 600; color: #fff;">当前没有任何正在执行的后台任务</div>
            <div style="font-size: 12px; margin-top: 4px;">所有命令和工具调用已完成或空闲</div>
          </div>
        `;
  } else {
    bodyHtml = `
          <div style="margin-bottom: 12px; font-size: 12.5px; color: var(--text-muted);">
            共检测到 <b style="color: var(--warning);">${terminalsState.runningCallsData.length}</b> 个正在运行的任务。支持强杀目标进程树并中断执行通道：
          </div>
          <div style="max-height: 400px; overflow-y: auto;">
            <table style="width: 100%; border-collapse: collapse; font-size: 12px; text-align: left;">
              <thead>
                <tr style="border-bottom: 1px solid var(--card-border); color: var(--text-muted);">
                  <th style="padding: 8px 10px;">目标终端</th>
                  <th style="padding: 8px 10px;">执行工具</th>
                  <th style="padding: 8px 10px;">已耗时</th>
                  <th style="padding: 8px 10px;">超时限制</th>
                  <th style="padding: 8px 10px; text-align: right;">操作</th>
                </tr>
              </thead>
              <tbody>
                ${terminalsState.runningCallsData
                  .map(
                    (c) => `
                  <tr style="border-bottom: 1px solid rgba(255,255,255,0.04);">
                    <td style="padding: 8px 10px;">
                      <span style="font-family: monospace; color: var(--primary); font-weight: 600;">${escapeHtml(c.terminal_id)}</span>
                    </td>
                    <td style="padding: 8px 10px;">
                      <span class="tag" style="padding: 2px 6px; font-size: 11px;">${escapeHtml(c.tool_name)}</span>
                    </td>
                    <td style="padding: 8px 10px; color: var(--warning); font-family: monospace;">
                      ${(c.elapsed_ms / 1000).toFixed(1)}s
                    </td>
                    <td style="padding: 8px 10px; color: var(--text-dim);">
                      ${c.timeout_secs}s
                    </td>
                    <td style="padding: 8px 10px; text-align: right;">
                      <button class="btn btn-sm" style="background: var(--danger); color: #fff; padding: 3px 8px; font-size: 11px;" onclick="cancelRunningCall('${escapeHtml(c.call_id)}')">⏹ 终止</button>
                    </td>
                  </tr>
                `,
                  )
                  .join("")}
              </tbody>
            </table>
          </div>
        `;
  }
  showModal("⚡ 运行中任务监控与应急阻断", bodyHtml);
}

export async function cancelRunningCall(callId) {
  if (!confirm(`确定要强制终止任务 [${callId}] 及其子进程树吗？`)) return;
  try {
    const res = await authFetch(restPaths.cancelCall(callId), { method: "POST" });
    const data = await res.json();
    if (data.success) {
      alert("已成功下发任务终止与进程树清理指令");
      await checkRunningCalls();
      showRunningCallsModal();
    } else {
      alert(`终止失败: ${data.error}`);
    }
  } catch (err) {
    alert(`请求异常: ${err.message}`);
  }
}

export function openEditMetaModal(terminalId) {
  const t = terminalsState.terminalsData.find(
    (item) => item.info?.terminal_id === terminalId,
  );
  if (!t) return alert("未找到指定终端信息");

  const currentName = t.custom_name || "";
  const currentNotes = t.notes || "";
  const currentTags = (t.tags || []).join(", ");
  const hostname = t.info?.hostname || "-";

  const formHtml = `
        <div style="display: flex; flex-direction: column; gap: 14px;">
          <div style="background: #080c14; padding: 12px 14px; border-radius: 8px; border: 1px solid var(--card-border); font-size: 12px; display: grid; grid-template-columns: 1fr 1fr; gap: 8px;">
            <div>终端编号 (ID): <b style="font-family: monospace; color: var(--primary);">${escapeHtml(terminalId)}</b></div>
            <div>原始主机名: <b style="font-family: monospace;">${escapeHtml(hostname)}</b></div>
            <div>内网 IP: <b style="font-family: monospace;">${escapeHtml(t.info?.lan_ip || "-")}</b></div>
            <div>操作系统: <b>${escapeHtml(t.info?.os_version || "-")}</b></div>
          </div>

          <div class="form-group">
            <label class="form-label" style="color: #f1f5f9; font-weight: 600;">
              🏷️ 终端自定义名称 / 别名 (Custom Name / Alias):
            </label>
            <input type="text" id="metaCustomName" class="form-control" placeholder="例如：财务-出纳主控机、研发-张三-工作站..." value="${escapeHtml(currentName)}" style="font-size: 13px;">
            <div style="font-size: 11px; color: var(--text-dim); margin-top: 4px;">便于控制台与 AI Agent 快速识别，保存后将在服务端持久化存储。若留空则自动降级显示原始主机名。</div>
          </div>

          <div class="form-group">
            <label class="form-label" style="color: #f1f5f9; font-weight: 600;">
              📝 运维详细备注 (Notes / Remarks):
            </label>
            <textarea id="metaNotes" class="form-control" rows="3" placeholder="例如：常驻3楼机房，配双千兆网卡，由运维组重点监控..." style="font-family: inherit; font-size: 12px; resize: vertical;">${escapeHtml(currentNotes)}</textarea>
          </div>

          <div class="form-group">
            <label class="form-label" style="color: #f1f5f9; font-weight: 600;">
              🏷️ 分组标签 (Tags, 多个以英文或中文逗号分隔):
            </label>
            <input type="text" id="metaTags" class="form-control" placeholder="例如：财务, Win11, 关键资产, 双网卡" value="${escapeHtml(currentTags)}" style="font-size: 12.5px;">
          </div>

          <div id="metaSaveResult"></div>

          <div style="display: flex; justify-content: flex-end; gap: 10px; margin-top: 8px;">
            <button class="btn btn-secondary" onclick="closeModal()">取消</button>
            <button class="btn btn-success" onclick="saveTerminalMeta('${escapeHtml(terminalId)}')">💾 保存更改并即时生效</button>
          </div>
        </div>
      `;

  showModal(`✏️ 编辑终端资产信息 · [${escapeHtml(terminalId)}]`, formHtml);
}

export async function saveTerminalMeta(terminalId) {
  const customName = byId("metaCustomName")?.value?.trim() || "";
  const notes = byId("metaNotes")?.value?.trim() || "";
  const rawTags = byId("metaTags")?.value?.trim() || "";
  const tags = parseTagList(rawTags);

  const resBox = byId("metaSaveResult");
  if (resBox) {
    resBox.innerHTML =
      '<div style="color: var(--primary); font-size: 12px;">正在保存持久化配置到服务端...</div>';
  }

  try {
    const res = await authFetch(restPaths.terminalMeta(terminalId), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        custom_name: customName,
        notes,
        tags,
      }),
    });
    const data = await res.json();
    if (data.success) {
      await fetchTerminals();
      closeModal();
    } else if (resBox) {
      resBox.innerHTML = `<div style="color: var(--danger); font-size: 12px;">保存失败: ${data.error || "未知错误"}</div>`;
    }
  } catch (err) {
    if (resBox) {
      resBox.innerHTML = `<div style="color: var(--danger); font-size: 12px;">请求异常: ${err.message}</div>`;
    }
  }
}

export async function deleteOfflineTerminal(terminalId) {
  if (!confirm(`确定要从平台注销并删除离线终端 [${terminalId}] 的资产记录吗？`)) return;
  try {
    const res = await authFetch(restPaths.terminal(terminalId), { method: "DELETE" });
    const data = await res.json();
    if (data.success) {
      await fetchTerminals();
    } else {
      alert(`删除失败: ${data.error || "未知错误"}`);
    }
  } catch (err) {
    alert(`请求异常: ${err.message}`);
  }
}

export async function runOverview(terminalId) {
  showModal(
    `终端 [${terminalId}] - 完整硬件概况`,
    "<pre>正在查询目标机器完整硬件及网络指标...</pre>",
  );
  try {
    const res = await authFetch(restPaths.invoke(terminalId), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ tool: "get_system_overview", arguments: {} }),
    });
    const data = await res.json();
    if (data.success) {
      showModal(
        `终端 [${terminalId}] - 完整硬件概况`,
        `<pre>${JSON.stringify(data.result, null, 2)}</pre>`,
      );
    } else {
      showModal(
        "查询失败",
        `<pre style="color: var(--danger)">${data.error || "Unknown error"}</pre>`,
      );
    }
  } catch (err) {
    showModal("请求错误", `<pre style="color: var(--danger)">${err.message}</pre>`);
  }
}

export async function runScreenshot(terminalId) {
  showModal(`终端 [${terminalId}] - 实时屏幕截图`, "<pre>正在请求屏幕截图...</pre>");
  try {
    const res = await authFetch(restPaths.invoke(terminalId), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ tool: "capture_screen", arguments: { quality: 80 } }),
    });
    const data = await res.json();
    const rawImg = data.result?.base64_data || data.result?.image_base64 || "";
    if (data.success && rawImg) {
      const imgUrl = rawImg.startsWith("data:")
        ? rawImg
        : `data:image/jpeg;base64,${rawImg}`;
      const w = data.result.width || "";
      const h = data.result.height || "";
      const dim = w && h ? ` (${w} × ${h})` : "";
      const html = `
            <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px;">
              <span style="font-size: 12px; color: var(--text-muted);">分辨率: <b>${w} × ${h}</b></span>
              <div style="display: flex; gap: 8px;">
                <a href="${imgUrl}" target="_blank" class="btn btn-secondary btn-sm">🔗 查看原图</a>
                <button class="btn btn-sm" onclick="runScreenshot('${terminalId}')">🔄 重新截取</button>
              </div>
            </div>
            <img src="${imgUrl}" class="screenshot-img" alt="Screenshot" />
          `;
      showModal(`终端 [${terminalId}] - 屏幕截图${dim}`, html);
    } else {
      showModal(
        "截图失败",
        `<pre style="color: var(--danger)">${data.error || JSON.stringify(data.result, null, 2)}</pre>`,
      );
    }
  } catch (err) {
    showModal("截图错误", `<pre style="color: var(--danger)">${err.message}</pre>`);
  }
}

export async function runProcessManager(terminalId) {
  showModal(`终端 [${terminalId}] - 进程管理器`, "<pre>正在加载运行进程列表...</pre>");
  try {
    const res = await authFetch(restPaths.invoke(terminalId), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        tool: "list_processes",
        arguments: { limit: 60, sort_by: "memory" },
      }),
    });
    const data = await res.json();
    if (!data.success) {
      showModal("查询失败", `<pre style="color: var(--danger)">${data.error}</pre>`);
      return;
    }

    const procs = Array.isArray(data.result) ? data.result : [];
    let html = `
          <div style="margin-bottom: 12px; display: flex; justify-content: space-between; align-items: center; gap: 10px;">
            <input type="text" id="procFilterInput" class="form-control" placeholder="🔍 快速过滤进程名称或 PID..." oninput="filterProcTable()" style="max-width: 320px;">
            <div style="font-size: 12px; color: var(--text-muted);">共 ${procs.length} 个进程 (按内存占用降序)</div>
          </div>
          <div style="max-height: 55vh; overflow-y: auto; border: 1px solid rgba(255,255,255,0.06); border-radius: 8px;">
            <table class="data-table" id="procTable">
              <thead>
                <tr>
                  <th style="width: 80px;">PID</th>
                  <th>进程名称</th>
                  <th style="width: 110px;">内存 (MB)</th>
                  <th style="width: 90px;">CPU (%)</th>
                  <th style="width: 90px; text-align: right;">操作</th>
                </tr>
              </thead>
              <tbody>
        `;

    html += procs.map((p) => processRowHtml(terminalId, p)).join("");

    html += `
              </tbody>
            </table>
          </div>
        `;
    showModal(`终端 [${terminalId}] - 进程管理器`, html);
  } catch (err) {
    showModal("请求错误", `<pre style="color: var(--danger)">${err.message}</pre>`);
  }
}

export function filterProcTable() {
  const q = (byId("procFilterInput")?.value || "")
    .toLowerCase()
    .trim();
  const rows = qsAll(".proc-row");
  rows.forEach((r) => {
    const name = r.getAttribute("data-name") || "";
    const pid = r.getAttribute("data-pid") || "";
    if (!q || name.includes(q) || pid.includes(q)) {
      r.style.display = "";
    } else {
      r.style.display = "none";
    }
  });
}

export async function killTargetProc(terminalId, pid, name) {
  if (!confirm(`确定要强制终止终端 [${terminalId}] 上的进程 ${name} (PID: ${pid}) 吗？`)) {
    return;
  }
  try {
    const res = await authFetch(restPaths.invoke(terminalId), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        tool: "kill_process",
        arguments: { pid, force: true },
      }),
    });
    const data = await res.json();
    if (data.success) {
      alert(`已终止进程 ${name} (${pid})`);
      runProcessManager(terminalId);
    } else {
      alert(`终止失败: ${data.error}`);
    }
  } catch (err) {
    alert(`请求异常: ${err.message}`);
  }
}

export function runServiceManager(terminalId) {
  const commonServices = [
    { name: "Spooler", desc: "Print Spooler (打印服务)" },
    { name: "wuauserv", desc: "Windows Update (自动更新)" },
    { name: "LanmanServer", desc: "Server (文件共享)" },
    { name: "EventLog", desc: "Windows Event Log (事件日志)" },
    { name: "Winmgmt", desc: "WMI (系统管理规范)" },
    { name: "Dhcp", desc: "DHCP Client (动态网络寻址)" },
    { name: "Dnscache", desc: "DNS Client (域名解析缓存)" },
  ];

  const html = `
        <div class="form-group">
          <label class="form-label">选择常用系统服务:</label>
          <select id="commonSvcSelect" class="form-control" onchange="document.getElementById('customSvcInput').value = this.value">
            <option value="">-- 请选择常见系统服务 --</option>
            ${commonServices.map((s) => `<option value="${s.name}">${s.name} - ${s.desc}</option>`).join("")}
          </select>
        </div>
        <div class="form-group">
          <label class="form-label">服务系统名称 (Service Name):</label>
          <div style="display: flex; gap: 8px;">
            <input type="text" id="customSvcInput" class="form-control" placeholder="例如: Spooler, wuauserv..." value="Spooler">
            <button class="btn btn-secondary" onclick="querySvcStatus('${terminalId}')">🔍 查询状态</button>
          </div>
        </div>
        <div style="display: flex; gap: 10px; margin-top: 14px;">
          <button class="btn btn-success" onclick="executeSvcAction('${terminalId}', 'start')">▶ 启动服务</button>
          <button class="btn btn-secondary" onclick="executeSvcAction('${terminalId}', 'restart')">🔄 重启服务</button>
          <button class="btn btn-danger" onclick="executeSvcAction('${terminalId}', 'stop')">⏹ 停止服务</button>
        </div>
        <div id="svcResultBox" style="margin-top: 16px;">
          <pre>输入服务名并点击【查询状态】查看详情...</pre>
        </div>
      `;
  showModal(`终端 [${terminalId}] - 系统服务管理`, html);
}

export async function querySvcStatus(terminalId) {
  const svcName = byId("customSvcInput")?.value?.trim();
  if (!svcName) return alert("请输入服务名称");
  const box = byId("svcResultBox");
  box.innerHTML = "<pre>正在查询服务状态...</pre>";
  try {
    const res = await authFetch(restPaths.invoke(terminalId), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        tool: "manage_service",
        arguments: { service_name: svcName, action: "status" },
      }),
    });
    const data = await res.json();
    if (data.success) {
      const s = data.result;
      const statusColor =
        s.status === "Running" || s.status === "active"
          ? "var(--success)"
          : "var(--warning)";
      box.innerHTML = `
            <div style="background: #080c14; padding: 14px; border-radius: 8px; border: 1px solid var(--card-border);">
              <div style="margin-bottom: 6px;">服务名称: <b>${s.name}</b> (${s.display_name || "-"})</div>
              <div style="margin-bottom: 6px;">运行状态: <b style="color: ${statusColor}">${s.status}</b></div>
              <div style="margin-bottom: 6px;">启动类型: <b>${s.start_type || "-"}</b></div>
              <div style="font-size: 12px; color: var(--text-muted);">${s.message || ""}</div>
            </div>
          `;
    } else {
      box.innerHTML = `<pre style="color: var(--danger)">${data.error}</pre>`;
    }
  } catch (err) {
    box.innerHTML = `<pre style="color: var(--danger)">${err.message}</pre>`;
  }
}

export async function executeSvcAction(terminalId, action) {
  const svcName = byId("customSvcInput")?.value?.trim();
  if (!svcName) return alert("请输入服务名称");
  if (
    !confirm(
      `确定要在终端 [${terminalId}] 上对服务 [${svcName}] 执行 [${action}] 操作吗？`,
    )
  ) {
    return;
  }
  const box = byId("svcResultBox");
  box.innerHTML = `<pre>正在执行 ${action}...</pre>`;
  try {
    const res = await authFetch(restPaths.invoke(terminalId), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        tool: "manage_service",
        arguments: { service_name: svcName, action },
      }),
    });
    const data = await res.json();
    if (data.success) {
      alert(`服务 ${svcName} ${action} 操作成功`);
      querySvcStatus(terminalId);
    } else {
      box.innerHTML = `<pre style="color: var(--danger)">操作失败: ${data.error}</pre>`;
    }
  } catch (err) {
    box.innerHTML = `<pre style="color: var(--danger)">请求异常: ${err.message}</pre>`;
  }
}

export function openCommandModal(terminalId) {
  const presets = [
    {
      label: "排查 1: 查看所有网卡与IP配置",
      cmd: "Get-NetIPAddress | Format-Table InterfaceAlias, IPAddress, AddressFamily",
    },
    {
      label: "排查 2: 查看本机所有监听端口 (Listen)",
      cmd: "Get-NetTCPConnection -State Listen | Select-Object LocalAddress, LocalPort, OwningProcess -First 15 | Format-Table",
    },
    {
      label: "排查 3: 检查所有磁盘可用空间",
      cmd: "Get-PSDrive -PSProvider 'FileSystem' | Select-Object Name, @{Name='UsedGB';Expression={[math]::Round($_.Used/1GB,2)}}, @{Name='FreeGB';Expression={[math]::Round($_.Free/1GB,2)}}",
    },
    {
      label: "排查 4: 内存占用 Top 8 进程",
      cmd: "Get-Process | Sort-Object WorkingSet64 -Descending | Select-Object -First 8 Name, Id, @{Name='MemoryMB';Expression={[math]::Round($_.WorkingSet64/1MB,1)}} | Format-Table",
    },
    {
      label: "排查 5: 查看系统近24小时重要错误事件",
      cmd: "Get-EventLog -LogName System -EntryType Error,Warning -Newest 6 | Format-Table TimeGenerated, Source, EventID, Message -AutoSize",
    },
  ];

  const html = `
        <div class="form-group">
          <label class="form-label">常用排障预设脚本 (Preset Runbooks):</label>
          <select id="cmdPresetSelect" class="form-control" onchange="document.getElementById('cmdScriptArea').value = this.value">
            <option value="Get-Process | Select-Object -First 5">-- 自定义 PowerShell 脚本 --</option>
            ${presets.map((p) => `<option value="${p.cmd.replace(/"/g, "&quot;")}">${p.label}</option>`).join("")}
          </select>
        </div>
        <div class="form-group">
          <label class="form-label">PowerShell 脚本内容:</label>
          <textarea id="cmdScriptArea" class="form-control" rows="6" style="resize: vertical;">Get-Process | Select-Object -First 5</textarea>
        </div>
        <div style="display: flex; justify-content: space-between; align-items: center; margin-top: 12px;">
          <div style="display: flex; align-items: center; gap: 8px; font-size: 12px; color: var(--text-muted);">
            超时时间: <input type="number" id="cmdTimeout" class="form-control" value="30" style="width: 70px;"> 秒
          </div>
          <button class="btn" onclick="executeCommand('${terminalId}')">⚡ 立即在后台静默执行</button>
        </div>
        <div id="cmdOutputBox" style="margin-top: 16px;"></div>
      `;
  showModal(`终端 [${terminalId}] - 执行运维诊断命令`, html);
}

export async function cancelTerminalCmd(terminalId) {
  const cancelBtn = byId("cancelCmdBtn");
  if (cancelBtn) {
    cancelBtn.disabled = true;
    cancelBtn.innerText = "正在终止...";
  }
  try {
    const res = await authFetch(restPaths.terminalCalls(terminalId));
    if (res.ok) {
      const data = await res.json();
      const calls = data.calls || [];
      if (calls.length > 0) {
        for (const call of calls) {
          await authFetch(restPaths.cancelCall(call.call_id), { method: "POST" });
        }
      }
    }
    const pre = byId("cmdPreOutput");
    if (pre) {
      pre.innerText =
        "已下发终止信号，正在强制杀除目标机器进程树并回收资源...";
    }
  } catch (err) {
    console.error("Cancel command failed:", err);
  }
}

export async function executeCommand(terminalId) {
  const script = byId("cmdScriptArea")?.value;
  if (!script) return alert("请输入脚本内容");
  const timeout = parseInt(byId("cmdTimeout")?.value || "30", 10);
  const outBox = byId("cmdOutputBox");

  let secondsElapsed = 0;
  outBox.innerHTML = `
        <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px; padding: 8px 12px; background: rgba(245, 158, 11, 0.1); border: 1px solid rgba(245, 158, 11, 0.3); border-radius: 6px;">
          <div style="display: flex; align-items: center; gap: 8px; color: var(--warning); font-size: 12.5px;">
            <span class="dot pulse" style="background: var(--warning); box-shadow: 0 0 8px var(--warning);"></span>
            <span>正在目标终端后台静默执行中... (<b id="cmdTimer">0</b>s)</span>
          </div>
          <button id="cancelCmdBtn" class="btn btn-sm" style="background: var(--danger); color: #fff; padding: 4px 10px; font-weight: 600;" onclick="cancelTerminalCmd('${terminalId}')">⏹ 终止任务</button>
        </div>
        <pre id="cmdPreOutput" style="opacity: 0.8; font-size: 12px;">命令已下发至终端，等待返回输出...</pre>
      `;

  if (terminalsState.activeCmdCancelPoll) {
    clearInterval(terminalsState.activeCmdCancelPoll);
  }
  terminalsState.activeCmdCancelPoll = setInterval(() => {
    secondsElapsed += 1;
    const timerSpan = byId("cmdTimer");
    if (timerSpan) timerSpan.innerText = String(secondsElapsed);
    checkRunningCalls();
  }, 1000);

  try {
    const res = await authFetch(restPaths.invoke(terminalId), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        tool: "exec_powershell",
        arguments: { script, timeout_secs: timeout },
        timeout_secs: timeout,
      }),
    });
    const data = await res.json();
    if (terminalsState.activeCmdCancelPoll) {
      clearInterval(terminalsState.activeCmdCancelPoll);
    }
    terminalsState.activeCmdCancelPoll = null;
    checkRunningCalls();

    if (data.success) {
      const exitCode = data.result?.exit_code;
      const out = data.result?.stdout || "(标准输出为空)";
      const err = data.result?.stderr ? `\n\n[STDERR]:\n${data.result.stderr}` : "";
      const statusStyle =
        exitCode === 0 ? "color: var(--success);" : "color: var(--danger);";
      outBox.innerHTML = `
            <div style="font-size: 12px; margin-bottom: 6px;">耗时: <b>${data.result?.duration_ms || secondsElapsed * 1000}ms</b> | 退出代码: <b style="${statusStyle}">${exitCode}</b></div>
            <pre>${out}${err}</pre>
          `;
    } else {
      const isCancelled = (data.error || "").toLowerCase().includes("cancel");
      const errMsg = isCancelled
        ? "⏹ 任务已被主动终止 (已强制杀除目标进程树)"
        : `执行失败: ${data.error}`;
      outBox.innerHTML = `<pre style="color: var(--danger); font-weight: 600;">${errMsg}</pre>`;
    }
  } catch (err) {
    if (terminalsState.activeCmdCancelPoll) {
      clearInterval(terminalsState.activeCmdCancelPoll);
    }
    terminalsState.activeCmdCancelPoll = null;
    checkRunningCalls();
    outBox.innerHTML = `<pre style="color: var(--danger)">请求异常: ${err.message}</pre>`;
  }
}

export { openRemoteDesktop };
