import { useEffect, useState } from 'react'
import api from '../lib/api'

const input = 'px-3 py-2 border rounded w-full'
const button = 'px-3 py-2 text-white bg-indigo-600 rounded disabled:opacity-50'

export default function StacksPage() {
  const [stacks, setStacks] = useState([])
  const [form, setForm] = useState({ slug: '', name: '', keywords: '' })
  const [editing, setEditing] = useState(null)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [busy, setBusy] = useState(false)
  const [reclassifyResult, setReclassifyResult] = useState(null)
  const [llm, setLlm] = useState({ enabled: false, base_url: '', api_key: '', model: 'deepseek-chat', timeout_secs: 20, has_key: false })
  useEffect(() => { api.get('/tech_stacks/llm-setting').then(r => setLlm({ ...r.data, api_key: '', has_key: (r.data.api_key_set ?? false) })).catch(() => {}) }, [])
  async function saveLlm() {
    setBusy(true); setError('')
    try { await api.put('/tech_stacks/llm-setting', llm); setLlm({ ...llm, api_key: '', has_key: llm.api_key ? true : llm.has_key }); setNotice('LLM 设置已保存') } catch (e) { setError(e.response?.data?.data?.message || '保存失败') } finally { setBusy(false) }
  }
  async function runReclassify(only) {
    setBusy(true); setError(''); setReclassifyResult(null)
    try {
      const r = await api.post('/tech_stacks/reclassify', { only })
      setReclassifyResult(r.data)
    } catch (e) {
      setError(e.response?.data?.data?.message || e.response?.data?.message || '重新判定失败')
    } finally { setBusy(false) }
  }

  async function refresh() {
    const r = await api.get('/tech_stacks')
    setStacks(r.data.data)
  }
  useEffect(() => { refresh().catch(() => setError('获取技术栈失败')) }, [])
  async function run(fn) {
    setBusy(true); setError('')
    try { await fn(); await refresh() }
    catch (e) { setError(e.response?.data?.data?.message || e.response?.data?.message || '操作失败') }
    finally { setBusy(false) }
  }

  return <div className="max-w-4xl mx-auto space-y-6">
    <div>
      <h1 className="text-2xl font-bold">技术栈管理</h1>
      {notice && <p className="mt-2 p-2 text-green-700 bg-green-50">{notice}</p>}
      <p className="mt-1 text-sm text-gray-600">漏洞入库时按关键词自动打技术栈标签；推送通道和接收群按订阅的栈接收。删除栈会自动从所有订阅中移除。</p>
    </div>
    {error && <p className="p-3 text-red-700 bg-red-50">{error}</p>}
    <section className="p-5 space-y-4 bg-white rounded shadow">
      <h2 className="font-semibold">{editing ? `编辑：${editing.slug}` : '新增技术栈'}</h2>
      <form className="grid gap-2 md:grid-cols-4" onSubmit={e => { e.preventDefault(); run(async () => {
        if (editing) {
          await api.put(`/tech_stacks/${editing.id}`, { slug: editing.slug, name: form.name, keywords: form.keywords, enabled: editing.enabled })
          setEditing(null)
        } else {
          await api.post('/tech_stacks', { ...form, enabled: true })
        }
        setForm({ slug: '', name: '', keywords: '' })
      }) }}>
        {!editing && <input className={input} placeholder="标识（如 php，字母数字-_）" value={form.slug} onChange={e => setForm({ ...form, slug: e.target.value })} required />}
        <input className={input} placeholder="显示名（如 PHP）" value={form.name} onChange={e => setForm({ ...form, name: e.target.value })} required />
        <textarea className={input + ' md:col-span-2 h-24 resize-y'} placeholder="关键词，逗号分隔（如 php,laravel,thinkphp,wordpress）" value={form.keywords} onChange={e => setForm({ ...form, keywords: e.target.value })} />
        <button className={button + ' self-start'} disabled={busy}>{editing ? '保存' : '新增'}</button>
      </form>
    </section>
    <section className="p-5 space-y-3 bg-white rounded shadow">
      <h2 className="font-semibold">LLM 智能判定</h2>
      <p className="text-sm text-gray-600">用大模型判断漏洞实际影响的技术栈。词典打标始终先行，LLM 失败自动回退词典结果。兼容 OpenAI 协议（DeepSeek/通义/GLM/Ollama）。</p>
      <form className="grid gap-2 md:grid-cols-5" onSubmit={e => { e.preventDefault(); saveLlm() }}>
        <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={llm.enabled} onChange={e => setLlm({ ...llm, enabled: e.target.checked })} /> 启用</label>
        <input className={input} placeholder="Base URL（https://api.deepseek.com/v1）" value={llm.base_url} onChange={e => setLlm({ ...llm, base_url: e.target.value })} />
        <input className={input} type="password" placeholder={llm.has_key ? 'API Key（已保存，留空不修改）' : 'API Key'} value={llm.api_key} onChange={e => setLlm({ ...llm, api_key: e.target.value })} />
        <input className={input} placeholder="模型（deepseek-chat）" value={llm.model} onChange={e => setLlm({ ...llm, model: e.target.value })} />
        <div className="flex gap-2"><input className={input} type="number" min="3" max="120" value={llm.timeout_secs} onChange={e => setLlm({ ...llm, timeout_secs: Number(e.target.value) })} /><button className={button} disabled={busy}>保存</button></div>
      </form>
      <div className="flex flex-wrap gap-2">
        <button className="px-3 py-2 border rounded text-indigo-700" disabled={busy} onClick={() => runReclassify('keyword')}>重判非 LLM 结果</button>
        <button className="px-3 py-2 border rounded text-indigo-700" disabled={busy} onClick={() => runReclassify('empty')}>只判未分类</button>
        <button className="px-3 py-2 border border-red-300 rounded text-red-700" disabled={busy} onClick={() => { if (window.confirm('全部存量重新判定？LLM 开启时会产生对应调用量。')) runReclassify('all') }}>全部重判</button>
      </div>
      {reclassifyResult && <p className="text-sm text-green-700">已处理 {reclassifyResult.processed} 条，标签变化 {reclassifyResult.updated} 条（范围：{reclassifyResult.scope}）。</p>}
    </section>
    <section className="p-5 space-y-2 bg-white rounded shadow">
      <h2 className="font-semibold">已配置（{stacks.length}）</h2>
      {stacks.map(t => <div key={t.id} className="p-3 border rounded">
        <div className="flex items-center justify-between">
          <span><b>{t.name}</b> <code className="text-xs text-gray-500">{t.slug}</code> — {t.enabled ? '启用' : '停用'}</span>
          <span className="flex gap-2">
            <button className="text-indigo-700" onClick={() => { setEditing(t); setForm({ slug: t.slug, name: t.name, keywords: t.keywords }) }}>编辑</button>
            <button className="text-indigo-700" onClick={() => run(() => api.put(`/tech_stacks/${t.id}`, { slug: t.slug, name: t.name, keywords: t.keywords, enabled: !t.enabled }))}>{t.enabled ? '停用' : '启用'}</button>
            <button className="text-red-700" onClick={() => { if (window.confirm(`删除技术栈“${t.name}”？会从所有订阅中移除。`)) run(() => api.delete(`/tech_stacks/${t.id}`)) }}>删除</button>
          </span>
        </div>
        <p className="mt-1 text-sm text-gray-500 break-all whitespace-pre-wrap">{t.keywords || '（无关键词）'}</p>
      </div>)}
    </section>
  </div>
}
