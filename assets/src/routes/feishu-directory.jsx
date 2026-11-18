import { useEffect, useMemo, useRef, useState } from 'react'
import api from '../lib/api'
import PeoplePicker from '../components/PeoplePicker'

const input = 'px-3 py-2 border rounded w-full text-sm focus:outline-none focus:ring-indigo-500 focus:border-indigo-500'
const button = 'px-3 py-1.5 text-white bg-indigo-600 rounded text-sm disabled:opacity-50 hover:bg-indigo-700'
const link = 'text-indigo-700 hover:underline'
const card = 'p-3 border border-gray-200 rounded-lg'

function SubHead({ title, hint }) {
  return (
    <div className="pt-2 border-t border-gray-200">
      <h3 className="text-sm font-semibold text-gray-900">{title}</h3>
      {hint && <p className="mt-0.5 text-xs text-gray-500">{hint}</p>}
    </div>
  )
}

export default function FeishuDirectory() {
  const [apps, setApps] = useState([])
  const [data, setData] = useState({ chats: [], groups: [] })
  const [app, setApp] = useState({ name: '', app_id: '', app_secret: '', tenant_key: '' })
  const [chat, setChat] = useState({ app_id: '', name: '', chat_id: '', stacks: [] })
  const [stacks, setStacks] = useState([])
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  async function refresh() { const [a, d] = await Promise.all([api.get('/receipts/apps'), api.get('/receipts/directory')]); setApps(a.data.data); setData(d.data) }
  const [people, setPeople] = useState([])
  const [members, setMembers] = useState(null)
  const [dims, setDims] = useState([])
  const [dimDraft, setDimDraft] = useState(null)
  async function refreshDims() { const r = await api.get('/receipts/dimensions'); setDims(r.data.data || []) }
  async function runDim(fn) { setBusy(true); setError(''); try { await fn(); await refreshDims() } catch (e) { setError(e.response?.data?.data?.message || '操作失败') } finally { setBusy(false) } }
  useEffect(() => { refreshDims().catch(() => {}) }, [])
  useEffect(() => { api.get('/receipts/people').then(r => setPeople(r.data.data || [])).catch(() => {}) }, [])
  // Full tenant directory (per-department, cached server-side); falls back to
  // receipt submitters only when the contact scope is missing.
  useEffect(() => { api.get('/receipts/directory/members').then(r => setMembers(r.data.data || [])).catch(() => setMembers([])) }, [])
  const contacts = Object.values(
    [...people.map(p => ({ open_id: p.open_id, name: p.name, department: '' })), ...(members || [])]
      .reverse()
      .reduce((acc, c) => { if (c.open_id) acc[c.open_id] = c; return acc }, {}),
  ).sort((a, b) => (a.name || '').localeCompare(b.name || '', 'zh'))
  // Lazily resolved names (server lookup) become first-class contact entries
  // so they also appear in PeoplePicker's name search, not just display lines.
  const [extraNames, setExtraNames] = useState({})
  const searchableContacts = useMemo(() => {
    const extras = Object.entries(extraNames)
      .filter(([id, name]) => name && !contacts.some(c => c.open_id === id))
      .map(([open_id, name]) => ({ open_id, name, department: '' }))
    return extras.length ? [...contacts, ...extras] : contacts
  }, [contacts, extraNames])
  useEffect(() => { api.get('/tech_stacks').then(r => setStacks((r.data.data || []).filter(t => t.enabled))).catch(() => {}) }, [])
  useEffect(() => { refresh().catch(() => setError('获取飞书配置失败')) }, [])
  async function run(fn) { setBusy(true); setError(''); try { await fn(); await refresh() } catch (e) { setError(e.response?.data?.data?.message || e.response?.data?.message || '操作失败，请检查配置和日志') } finally { setBusy(false) } }
  const options = (rows, label) => rows.map(r => <option key={r.id} value={r.id}>{label(r)}</option>)
  // IDs referenced by mentions/dimension owners but absent from both contacts
  // sources (never submitted a receipt, not in the directory pull) are
  // resolved on demand via the per-user contact API and cached for the session.
  const requestedRef = useRef(new Set())
  const allReferencedIds = useMemo(() => {
    const ids = new Set()
    for (const c of (data.chats || [])) (c.mention_open_ids || '').split(',').forEach(id => id.trim().startsWith('ou_') && ids.add(id.trim()))
    for (const d of dims) if (d.owner_open_id) ids.add(d.owner_open_id.trim())
    return [...ids]
  }, [data.chats, dims])
  useEffect(() => {
    for (const id of allReferencedIds) {
      if (contacts.some(p => p.open_id === id) || requestedRef.current.has(id)) continue
      requestedRef.current.add(id)
      api.get(`/receipts/people/${encodeURIComponent(id)}`)
        .then(r => setExtraNames(prev => ({ ...prev, [id]: r.data.name || id })))
        .catch(() => setExtraNames(prev => ({ ...prev, [id]: id })))
    }
  }, [allReferencedIds, contacts])
  const personName = (openId) => {
    const id = (openId || '').trim()
    return contacts.find(p => p.open_id === id)?.name || extraNames[id]
  }
  const [mentionDraft, setMentionDraft] = useState(null)
  async function resolveOpenId(value) {
    const openId = (value || '').trim()
    setMentionDraft(d => ({ ...d, openId, resolved: null, error: '' }))
    if (!openId.startsWith('ou_')) return
    try {
      const r = await api.get(`/receipts/people/${encodeURIComponent(openId)}`)
      setMentionDraft(d => ({ ...d, resolved: r.data }))
    } catch (e) {
      setMentionDraft(d => ({ ...d, error: e.response?.data?.data?.message || '查询失败' }))
    }
  }
  const queryHits = (mentionDraft?.query || '').trim() ? contacts.filter(p => (p.name || '').toLowerCase().includes(mentionDraft.query.trim().toLowerCase())).slice(0, 8) : []
  return <div className="space-y-5">
    {error && <p className="p-2 text-sm text-red-700 bg-red-50 rounded">{error}</p>}

    <SubHead title="飞书应用" hint="企业自建应用凭据；卡片与催办均通过启用的应用发送" />
    <form className="grid gap-2 md:grid-cols-5" onSubmit={e => { e.preventDefault(); run(async () => { await api.post('/receipts/apps', app); setApp({ name:'', app_id:'', app_secret:'', tenant_key:'' }) }) }}>
      {['name','app_id','app_secret','tenant_key'].map((k,i) => <input key={k} className={input} required={k !== 'tenant_key'} type={k === 'app_secret' ? 'password' : 'text'} placeholder={['应用名称','App ID','App Secret','Tenant Key（可留空）'][i]} value={app[k]} onChange={e => setApp({ ...app, [k]:e.target.value })} />)}<button className={button} disabled={busy}>添加应用</button>
    </form>
    {apps.map(a => <p key={a.id} className="text-sm">{a.name}（{a.app_id}）— {a.enabled ? '启用' : '停用'} <button className={link} onClick={() => run(() => api.put(`/receipts/apps/${a.id}`, { ...a, enabled: !a.enabled }))}>{a.enabled ? '停用' : '启用'}</button> <button className={link} onClick={() => { const name = window.prompt('应用名称', a.name); if (name) run(() => api.put(`/receipts/apps/${a.id}`, { ...a, name })) }}>改名</button> <button className={link} onClick={() => { const secret = window.prompt('输入新 App Secret'); if (secret) run(() => api.put(`/receipts/apps/${a.id}`, { ...a, app_secret:secret })) }}>更换密钥</button></p>)}

    <SubHead title="接收群" hint="确认卡片发送到所有启用的群；可按技术栈订阅过滤" />
    <form className="grid gap-2 md:grid-cols-4" onSubmit={e => { e.preventDefault(); run(async () => { await api.post('/receipts/directory/chats', { ...chat, stacks: (chat.stacks || []).join(','), app_id:Number(chat.app_id) }); setChat({ ...chat, name:'', chat_id:'', stacks: [] }) }) }}><select className={input} required value={chat.app_id} onChange={e => setChat({ ...chat, app_id:e.target.value })}><option value="">选择应用</option>{options(apps.filter(a=>a.enabled), a=>a.name)}</select><input className={input} placeholder="群名称" required value={chat.name} onChange={e=>setChat({...chat,name:e.target.value})}/><input className={input} placeholder="群 Chat ID（oc_）" required value={chat.chat_id} onChange={e=>setChat({...chat,chat_id:e.target.value})}/><div className="sm:col-span-4 flex flex-wrap gap-3 items-center"><span className="text-sm text-gray-600">订阅：</span>{stacks.map(t => <label key={t.slug} className="inline-flex items-center gap-1 text-sm"><input type="checkbox" checked={(chat.stacks||[]).includes(t.slug)} onChange={e=>setChat({...chat, stacks: e.target.checked?[...(chat.stacks||[]),t.slug]:(chat.stacks||[]).filter(s=>s!==t.slug)})}/>{t.name}</label>)}{stacks.length===0&&<span className="text-xs text-gray-400">先配置技术栈</span>}</div>
<button className={button} disabled={busy}>添加群</button></form>
    {data.chats.map(c => <div key={c.id} className={card}>
      <p className="text-sm">{c.name}（{c.chat_id} · {apps.find(a=>a.id===c.app_id)?.name}）— {c.enabled ? '启用' : '停用'} <button className={link} onClick={()=>run(()=>api.put(`/receipts/directory/chats/${c.id}`,{enabled:!c.enabled}))}>{c.enabled?'停用':'启用'}</button> <button className={link} onClick={()=>{const name=window.prompt('群名称',c.name);if(name)run(()=>api.put(`/receipts/directory/chats/${c.id}`,{name}))}}>改名</button></p>
      <p className="mt-1 text-xs text-gray-500">订阅：{c.stacks ? c.stacks.split(',').join('、') : '（全部）'}</p>
      <p className="text-xs text-gray-500">@ 提醒：{(c.mention_open_ids || '').split(',').filter(Boolean).map(id => personName(id) || id).join('、') || '（无）'}</p>
      <div className="flex flex-wrap gap-3 mt-1">
        {/* Checkboxes cover everyone already mentioned plus receipt submitters,
            so dialog-added people can be unchecked like any other. */}
        {(() => {
          const mentioned = (c.mention_open_ids || '').split(',').filter(Boolean)
          const extra = mentioned.filter(id => !people.some(p => p.open_id === id))
            .map(id => ({ open_id: id, name: personName(id) || id }))
          return [...people, ...extra].map(p => (
            <label key={p.open_id} className="inline-flex items-center gap-1 text-sm">
              <input type="checkbox" checked={mentioned.includes(p.open_id)} onChange={e => run(async () => {
                const cur = (c.mention_open_ids || '').split(',').filter(Boolean)
                const next = e.target.checked ? [...cur, p.open_id] : cur.filter(id => id !== p.open_id)
                await api.put(`/receipts/directory/chats/${c.id}`, { mention_open_ids: next.join(',') })
              })} />
              @{p.name}
            </label>
          ))
        })()}
        <button className={link + ' text-xs'} onClick={() => setMentionDraft({ chat: c, openId: '', resolved: null, query: '', error: '' })}>+ 添加人员</button>
      </div>
      <div className="flex flex-wrap gap-3 mt-1">
        {stacks.map(t => (
          <label key={t.slug} className="inline-flex items-center gap-1 text-sm">
            <input type="checkbox" checked={(c.stacks || '').split(',').includes(t.slug)} onChange={e => run(async () => {
              const cur = (c.stacks || '').split(',').filter(Boolean)
              const next = e.target.checked ? [...cur, t.slug] : cur.filter(s => s !== t.slug)
              await api.put(`/receipts/directory/chats/${c.id}`, { stacks: next.join(',') })
            })} />
            {t.name}
          </label>
        ))}
        {stacks.length === 0 && <span className="text-xs text-gray-400">先在「技术栈」页配置</span>}
      </div>
    </div>)}

    <SubHead title="回执维度（三维度确认）" hint="催办私发到负责人单聊（配了 Open ID 时）；三维度全部完成回执后才能关闭确认单" />
    {dims.map(d => {
      if (dimDraft?.id === d.id) {
        const draft = dimDraft
        return <div key={d.id} className="p-3 border border-indigo-200 rounded-lg bg-indigo-50/40 space-y-2">
          <div className="grid gap-3 md:grid-cols-2">
            <label className="text-xs text-gray-600">维度名称<input className={input + ' mt-1'} value={draft.name} onChange={e => setDimDraft({ ...draft, name: e.target.value })} /></label>
            <div className="text-xs text-gray-600">
              负责人（催办私发对象）
              <div className="mt-1">
                <PeoplePicker contacts={searchableContacts} value={{ open_id: draft.owner_open_id, name: draft.owner_name }}
                  onChange={picked => setDimDraft({ ...draft, owner_open_id: picked.open_id, owner_name: picked.name })} />
              </div>
              {!draft.owner_open_id.trim() && <p className="mt-1 text-amber-600">未选择负责人：催办将走群 @</p>}
            </div>
          </div>
          <div className="flex gap-2">
            <button className={button} disabled={busy || !draft.name.trim()} onClick={() => runDim(async () => {
              await api.put(`/receipts/dimensions/${d.id}`, {
                name: draft.name.trim(),
                owner_name: draft.owner_name.trim(),
                owner_open_id: draft.owner_open_id.trim(),
                enabled: d.enabled,
              })
              setDimDraft(null)
            })}>保存</button>
            <button className="px-3 py-1.5 text-sm text-gray-600 border rounded hover:bg-gray-50" onClick={() => setDimDraft(null)}>取消</button>
          </div>
        </div>
      }
      return <div key={d.id} className={card + ' text-sm' + (d.enabled ? '' : ' opacity-60')}>
        <div className="flex items-center justify-between gap-2 flex-wrap">
          <p><b>{d.name}</b>（{d.slug}）— 负责人：{d.owner_name || '未设置'}
            {d.owner_open_id && <span className="text-xs text-gray-500"> · {personName(d.owner_open_id) || d.owner_open_id}</span>}
          </p>
          <div className="flex gap-3">
            <button className={link} onClick={() => setDimDraft({ id: d.id, name: d.name, owner_name: d.owner_name, owner_open_id: d.owner_open_id })}>编辑</button>
            <button className={link} onClick={() => runDim(() => api.put(`/receipts/dimensions/${d.id}`, { name: d.name, owner_name: d.owner_name, owner_open_id: d.owner_open_id, enabled: !d.enabled }))}>{d.enabled ? '停用' : '启用'}</button>
          </div>
        </div>
      </div>
    })}
      {mentionDraft && <div className="fixed inset-0 z-50 bg-black/50 p-4 overflow-y-auto" onClick={() => setMentionDraft(null)}>
        <div className="max-w-lg mx-auto my-16 p-5 bg-white rounded shadow space-y-3" onClick={e => e.stopPropagation()}>
          <h3 className="font-semibold">添加 @ 提醒人员 — {mentionDraft.chat.name}</h3>
          <div>
            <label className="text-xs text-gray-500">按 Open ID 添加（自动查姓名）</label>
            <div className="flex gap-2">
              <input className={input} placeholder="ou_ 开头的 Open ID" value={mentionDraft.openId} onChange={e => resolveOpenId(e.target.value)} />
              <button className={button} disabled={!(mentionDraft.resolved || mentionDraft.openId.startsWith('ou_'))} onClick={() => run(async () => {
                const cur = (mentionDraft.chat.mention_open_ids || '').split(',').filter(Boolean)
                const next = [...new Set([...cur, mentionDraft.openId.trim()])].join(',')
                await api.put(`/receipts/directory/chats/${mentionDraft.chat.id}`, { mention_open_ids: next })
                setMentionDraft(null)
              })}>添加</button>
            </div>
            {mentionDraft.resolved && <p className="text-xs text-green-700">✓ {mentionDraft.resolved.name}{(mentionDraft.resolved.departments || []).length > 0 ? ` · ${(mentionDraft.resolved.departments || []).join('、')}` : ''}</p>}
            {mentionDraft.error && <p className="text-xs text-red-700">{mentionDraft.error}</p>}
          </div>
          <div>
            <label className="text-xs text-gray-500">按姓名搜索通讯录</label>
            <input className={input} placeholder="输入姓名关键字" value={mentionDraft.query} onChange={e => setMentionDraft(d => ({ ...d, query: e.target.value }))} />
            <div className="mt-2 space-y-1 max-h-48 overflow-y-auto">
              {queryHits.map(p => <button key={p.open_id} className="block w-full text-left px-3 py-2 text-sm border rounded hover:bg-gray-50" onClick={() => run(async () => {
                const cur = (mentionDraft.chat.mention_open_ids || '').split(',').filter(Boolean)
                const next = [...new Set([...cur, p.open_id])].join(',')
                await api.put(`/receipts/directory/chats/${mentionDraft.chat.id}`, { mention_open_ids: next })
                setMentionDraft(null)
              })}>{p.name} <span className="text-xs text-gray-400">{p.open_id}</span></button>)}
              {(mentionDraft.query || '').trim() && queryHits.length === 0 && <p className="text-xs text-gray-400">通讯录中无匹配（可先按 Open ID 添加，或等通讯录缓存刷新）</p>}
            </div>
          </div>
          <button className={link} onClick={() => setMentionDraft(null)}>关闭</button>
        </div>
      </div>}
  </div>
}
