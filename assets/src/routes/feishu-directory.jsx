import { useEffect, useState } from 'react'
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
  useEffect(() => { api.get('/tech_stacks').then(r => setStacks((r.data.data || []).filter(t => t.enabled))).catch(() => {}) }, [])
  useEffect(() => { refresh().catch(() => setError('获取飞书配置失败')) }, [])
  async function run(fn) { setBusy(true); setError(''); try { await fn(); await refresh() } catch (e) { setError(e.response?.data?.data?.message || e.response?.data?.message || '操作失败，请检查配置和日志') } finally { setBusy(false) } }
  const options = (rows, label) => rows.map(r => <option key={r.id} value={r.id}>{label(r)}</option>)
  const personName = (openId) => contacts.find(p => p.open_id === (openId || '').trim())?.name
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
        {people.map(p => (
          <label key={p.open_id} className="inline-flex items-center gap-1 text-sm">
            <input type="checkbox" checked={(c.mention_open_ids || '').split(',').filter(Boolean).includes(p.open_id)} onChange={e => run(async () => {
              const cur = (c.mention_open_ids || '').split(',').filter(Boolean)
              const next = e.target.checked ? [...cur, p.open_id] : cur.filter(id => id !== p.open_id)
              await api.put(`/receipts/directory/chats/${c.id}`, { mention_open_ids: next.join(',') })
            })} />
            @{p.name}
          </label>
        ))}
        <button className={link + ' text-xs'} onClick={() => {
          const added = window.prompt('追加 Open ID（ou_ 开头，多个逗号分隔）')
          if (added) run(async () => {
            const cur = (c.mention_open_ids || '').split(',').filter(Boolean)
            const add = added.split(',').map(x => x.trim()).filter(x => x.startsWith('ou_'))
            await api.put(`/receipts/directory/chats/${c.id}`, { mention_open_ids: [...new Set([...cur, ...add])].join(',') })
          })
        }}>+ 手动追加</button>
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
                <PeoplePicker contacts={contacts} value={{ open_id: draft.owner_open_id, name: draft.owner_name }}
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
  </div>
}
