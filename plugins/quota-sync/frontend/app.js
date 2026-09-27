const $ = selector => document.querySelector(selector)
const decoder = new TextDecoder()

let snapshot = null
let draft = { dryRun: true, mappings: [] }
let selectedAccountId = null

async function api(method, path, data) {
  const reply = await window.codexProxyPlugin.request({
    method,
    path,
    ...(data === undefined ? {} : {
      contentType: 'application/json',
      body: JSON.stringify(data),
    }),
  })
  const contentType = reply.contentType.split(';', 1)[0]?.trim().toLowerCase()
  if (contentType !== 'application/json')
    throw new Error('插件返回了不支持的内容类型')
  const value = JSON.parse(decoder.decode(reply.body))
  if (reply.status < 200 || reply.status >= 300)
    throw new Error(value?.error?.message || \`插件请求失败（HTTP ${reply.status}）\`)
  return value
}

function notice(message, error = false) {
  const el = $('#notice')
  el.hidden = !message
  el.textContent = message || ''
  el.classList.toggle('error', error)
}

function mapping(accountId, create = false) {
  let value = draft.mappings.find(item => item.accountId === accountId)
  if (!value && create) {
    value = { accountId, clientKeyIds: [], quotaWindowKey: null }
    draft.mappings.push(value)
  }
  return value
}

function ownerOfKey(keyId) {
  return draft.mappings.find(item => item.clientKeyIds.includes(keyId))?.accountId || null
}

function weeklyWindows(account) {
  return (account.quota?.windows || [])
    .filter(window => window.window_seconds === snapshot.weeklyWindowSeconds)
}

function keyScopeAllowsAccount(key, account) {
  if (!Array.isArray(key.groupIds))
    return null
  if (key.groupIds.length === 0)
    return true
  const accountGroups = new Set(account.groupIds || [])
  return key.groupIds.some(groupId => accountGroups.has(groupId))
}

function fmtTime(ms) {
  return Number.isFinite(ms) ? new Date(ms).toLocaleString() : '未知'
}

function fmtPercent(value) {
  return Number.isFinite(value) ? \`${value.toFixed(1)}%\` : '未知'
}

function renderAccounts() {
  const root = $('#accounts')
  root.replaceChildren()
  if (!snapshot.accounts.length) {
    root.className = 'account-list empty'
    root.textContent = '没有可用的 OpenAI 账号'
    return
  }
  root.className = 'account-list'
  for (const account of snapshot.accounts) {
    const windows = weeklyWindows(account)
    const selected = account.accountId === selectedAccountId
    const map = mapping(account.accountId)
    const card = document.createElement('div')
    card.className = \`account${selected ? ' active' : ''}\`
    card.addEventListener('click', () => {
      selectedAccountId = account.accountId
      render()
    })

    const top = document.createElement('div')
    top.className = 'row'
    const id = document.createElement('strong')
    id.className = 'mono'
    id.textContent = account.accountId
    const badge = document.createElement('span')
    badge.className = 'badge'
    badge.textContent = \`${map?.clientKeyIds.length || 0} 个 Key\`
    top.append(id, badge)

    const meta = document.createElement('div')
    meta.className = 'meta'
    const weekly = windows[0]
    const usage = document.createElement('span')
    usage.textContent = \`周用量：${weekly ? fmtPercent(weekly.used_percent) : '无数据'}\`
    const reset = document.createElement('span')
    reset.textContent = \`上游重置：${weekly ? fmtTime(weekly.reset_at_ms) : '未知'}\`
    const observed = document.createElement('span')
    observed.textContent = \`观测：${fmtTime(account.quota?.observed_at_ms)}\`
    meta.append(usage, reset, observed)
    if (!account.enabled) {
      const disabled = document.createElement('span')
      disabled.className = 'badge warn'
      disabled.textContent = '账号停用'
      meta.append(disabled)
    }
    if (account.runtime?.pending) {
      const pending = document.createElement('span')
      pending.className = 'badge warn'
      pending.textContent = '等待二次确认'
      meta.append(pending)
    }
    if (account.runtime?.lastError) {
      const error = document.createElement('span')
      error.className = 'badge error'
      error.textContent = account.runtime.lastError
      meta.append(error)
    }
    card.append(top, meta)
    root.append(card)
  }
}

function renderWindowPicker(account) {
  const root = $('#window-picker')
  const windows = weeklyWindows(account)
  const map = mapping(account.accountId)
  if (windows.length <= 1) {
    root.hidden = true
    return
  }
  root.hidden = false
  root.replaceChildren()
  const label = document.createElement('label')
  label.textContent = '周窗口：'
  const select = document.createElement('select')
  const placeholder = document.createElement('option')
  placeholder.value = ''
  placeholder.textContent = '请选择'
  select.append(placeholder)
  for (const window of windows) {
    const option = document.createElement('option')
    option.value = window.key
    option.textContent = \`${window.key} · ${fmtPercent(window.used_percent)} · ${fmtTime(window.reset_at_ms)}\`
    option.selected = map?.quotaWindowKey === window.key
    select.append(option)
  }
  select.addEventListener('change', () => {
    const target = mapping(account.accountId, true)
    target.quotaWindowKey = select.value || null
  })
  label.append(select)
  root.append(label)
}

function renderKeys() {
  const root = $('#keys')
  const account = snapshot.accounts.find(item => item.accountId === selectedAccountId)
  $('#refresh-account').disabled = !account
  if (!account) {
    $('#selected-account').textContent = '尚未选择账号'
    $('#window-picker').hidden = true
    root.className = 'key-list empty'
    root.textContent = '请选择左侧账号'
    return
  }
  $('#selected-account').textContent = account.accountId
  renderWindowPicker(account)
  root.replaceChildren()
  root.className = 'key-list'
  if (!snapshot.keys.length) {
    root.className = 'key-list empty'
    root.textContent = '没有可用的 Client Key'
    return
  }

  const selected = new Set(mapping(account.accountId)?.clientKeyIds || [])
  for (const key of snapshot.keys) {
    const isSelected = selected.has(key.id)
    const scopeAllows = keyScopeAllowsAccount(key, account)
    const currentOwner = ownerOfKey(key.id)
    const ownedElsewhere = currentOwner && currentOwner !== account.accountId

    const row = document.createElement('label')
    row.className = \`key${scopeAllows === false ? ' scope-mismatch' : ''}\`
    const checkbox = document.createElement('input')
    checkbox.type = 'checkbox'
    checkbox.checked = isSelected
    checkbox.disabled = !isSelected && (scopeAllows === false || ownedElsewhere)
    checkbox.addEventListener('change', () => {
      const target = mapping(account.accountId, true)
      const values = new Set(target.clientKeyIds)
      if (checkbox.checked)
        values.add(key.id)
      else
        values.delete(key.id)
      target.clientKeyIds = [...values]
      render()
    })

    const body = document.createElement('div')
    const top = document.createElement('div')
    top.className = 'row'
    const name = document.createElement('strong')
    name.textContent = key.name
    const state = document.createElement('span')
    state.className = key.enabled ? 'badge' : 'badge warn'
    state.textContent = key.enabled ? '启用' : '停用'
    top.append(name, state)

    const meta = document.createElement('div')
    meta.className = 'meta'
    const id = document.createElement('span')
    id.className = 'mono'
    id.textContent = key.id
    const budget = document.createElement('span')
    if (key.budget) {
      const limit = key.budget.weekly_limit_usd === '0' ? '不限' : \`$${key.budget.weekly_limit_usd}\`
      budget.textContent = \`周额度：$${key.budget.weekly_used_usd} / ${limit}\`
    }
    else {
      budget.textContent = '预算不可用'
    }
    const scope = document.createElement('span')
    if (scopeAllows === null) {
      scope.className = 'badge warn'
      scope.textContent = '范围未知'
    }
    else if (scopeAllows === false) {
      scope.className = 'badge error'
      scope.textContent = '范围不含此账号'
    }
    else if ((key.groupIds || []).length === 0) {
      scope.className = 'badge warn'
      scope.textContent = 'AllAccounts'
    }
    else {
      scope.className = 'badge success'
      scope.textContent = '账号组匹配'
    }
    meta.append(id, budget, scope)

    if (ownedElsewhere) {
      const owner = document.createElement('span')
      owner.className = 'badge warn'
      owner.textContent = \`已关联 ${currentOwner}\`
      meta.append(owner)
    }

    body.append(top, meta)
    row.append(checkbox, body)
    root.append(row)
  }
}

const outcomeLabels = {
  prepared: '需人工确认',
  dry_run: 'Dry Run',
  completed: '完成',
  partial_failure: '部分失败',
}

const keyStatusLabels = {
  pending: '未确认',
  dry_run: 'Dry Run',
  reset: '已重置',
  scope_mismatch: '范围失配',
  facts_unavailable: 'Key 状态不可用',
  failed_unknown: '结果未知',
}

function renderEvents() {
  const root = $('#events')
  root.replaceChildren()
  const events = [...snapshot.events].reverse().slice(0, 20)
  if (!events.length) {
    root.className = 'events empty'
    root.textContent = '暂无事件'
    return
  }
  root.className = 'events'
  for (const event of events) {
    const item = document.createElement('div')
    item.className = 'event'
    const top = document.createElement('div')
    top.className = 'row'
    const title = document.createElement('strong')
    title.textContent = event.accountId
    const outcome = document.createElement('span')
    const problem = event.outcome === 'partial_failure' || event.outcome === 'prepared'
    outcome.className = \`badge${problem ? ' error' : event.outcome === 'completed' ? ' success' : ''}\`
    outcome.textContent = outcomeLabels[event.outcome] || event.outcome
    top.append(title, outcome)

    const meta = document.createElement('div')
    meta.className = 'meta'
    meta.textContent = \`${fmtTime(event.detectedAtMs)} · ${event.kind} · ${fmtPercent(event.previousUsedPercent)} → ${fmtPercent(event.currentUsedPercent)}\`

    const keys = document.createElement('div')
    keys.className = 'event-keys'
    for (const result of event.keys) {
      const key = document.createElement('span')
      key.className = 'event-key mono'
      key.textContent = \`${result.keyId}: ${keyStatusLabels[result.status] || result.status}\`
      keys.append(key)
    }
    item.append(top, meta, keys)
    root.append(item)
  }
}

function render() {
  $('#dry-run').checked = draft.dryRun
  renderAccounts()
  renderKeys()
  renderEvents()
}

async function load() {
  notice('')
  try {
    snapshot = await api('GET', 'api/snapshot')
    draft = structuredClone(snapshot.settings)
    if (!selectedAccountId || !snapshot.accounts.some(item => item.accountId === selectedAccountId))
      selectedAccountId = snapshot.accounts[0]?.accountId || null
    render()
  }
  catch (error) {
    notice(error.message, true)
  }
}

$('#dry-run').addEventListener('change', event => {
  draft.dryRun = event.target.checked
})

$('#reload').addEventListener('click', load)

$('#save').addEventListener('click', async () => {
  notice('')
  const mappings = draft.mappings
    .filter(item => item.clientKeyIds.length > 0)
    .map(item => ({
      accountId: item.accountId,
      clientKeyIds: [...new Set(item.clientKeyIds)],
      ...(item.quotaWindowKey ? { quotaWindowKey: item.quotaWindowKey } : {}),
    }))
  try {
    await api('POST', 'api/settings', { dryRun: draft.dryRun, mappings })
    notice('设置已保存。维护任务会在下一轮按新映射执行。')
    await load()
  }
  catch (error) {
    notice(error.message, true)
  }
})

$('#refresh-account').addEventListener('click', async () => {
  if (!selectedAccountId)
    return
  notice('')
  const button = $('#refresh-account')
  button.disabled = true
  try {
    await api('POST', 'api/refresh-account', { accountId: selectedAccountId })
    notice('额度观测已刷新。')
    await load()
  }
  catch (error) {
    notice(error.message, true)
  }
  finally {
    button.disabled = false
  }
})

$('#clear-events').addEventListener('click', async () => {
  notice('')
  const button = $('#clear-events')
  button.disabled = true
  try {
    await api('POST', 'api/clear-events', '')
    notice('事件记录已清空。')
    await load()
  }
  catch (error) {
    notice(error.message, true)
  }
  finally {
    button.disabled = false
  }
})

load()
