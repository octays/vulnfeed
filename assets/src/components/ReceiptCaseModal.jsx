import { useEffect, useState } from 'react'
import api from '../lib/api'
import { localDateTime } from '../lib/format'
import { TASK_STATUSES } from '../lib/receipt'
import IndividualResponses from './IndividualResponses'

const input = 'px-3 py-2 border rounded w-full'
const button = 'px-3 py-2 text-white bg-indigo-600 rounded disabled:opacity-50'

// Receipt-case detail modal shared by the receipts page and the vuln/notice lists.
export default function ReceiptCaseModal({ caseId, onClose, onChanged }) {
  const [detail, setDetail] = useState(null)
  const [dimensions, setDimensions] = useState([])
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [closeNote, setCloseNote] = useState('')

  async function load() {
    const r = await api.get(`/receipts/cases/${caseId}`)
    setDetail(r.data)
  }
  useEffect(() => {
    setDetail(null); setError('')
    load().catch(e => setError(e.response?.data?.data?.message || '获取确认单失败'))
    api.get('/receipts/dimensions').then(r => setDimensions(r.data.data || [])).catch(() => {})
  }, [caseId])
  async function run(fn) {
    setBusy(true); setError('')
    try { await fn(); await load(); if (onChanged) { try { await onChanged() } catch (_) { /* 列表刷新失败不影响弹窗 */ } } }
    catch (e) { setError(e.response?.data?.data?.message || e.response?.data?.message || e.message || '操作失败，请检查配置和日志') }
    finally { setBusy(false) }
  }
  const refresh = () => load()
  const exportCsv = () => run(async () => {
    const res = await api.get(`/receipts/cases/${caseId}/export`, { responseType: 'blob' })
    const url = URL.createObjectURL(res.data)
    const a = document.createElement('a')
    a.href = url; a.download = `确认单-${caseId}.csv`; a.click(); URL.revokeObjectURL(url)
  })

  return <div className="fixed inset-0 z-50 bg-black/50 p-4 overflow-y-auto" onClick={onClose}>
    <section className="max-w-4xl mx-auto my-8 p-5 space-y-3 bg-white rounded shadow" onClick={e => e.stopPropagation()}>
      <div className="flex items-start justify-between gap-3">
        <h2 className="text-lg font-semibold">#{caseId} {detail ? `${detail.title} — ${TASK_STATUSES[detail.status] || detail.status}` : '加载中…'}</h2>
        <button className="px-2 py-1 text-gray-500 border rounded hover:text-gray-700" onClick={onClose}>关闭 ✕</button>
      </div>
      {error && <p className="p-2 text-red-700 bg-red-50">{error}</p>}
      {detail && <>
        {detail.tasks.map(t => <div key={t.id} className="space-y-2 border-b py-3">
          <div className="flex items-center justify-between">
            <span>{TASK_STATUSES[t.status] || t.status}（维度 {t.dims_done}/{t.dims_total}） {t.refresh_failed && <span className="text-red-700">（飞书卡片更新失败，系统将重试）</span>}</span>
            {detail.status === 'open' && <div className="flex gap-2">
              <button className="px-3 py-2 border rounded disabled:opacity-50" disabled={busy} title="向未回执维度的负责人发催办卡片" onClick={() => run(() => api.post(`/receipts/tasks/${t.id}/remind`))}>催办未回执维度</button>
              <button className={button} disabled={busy} onClick={() => run(() => api.post(`/receipts/tasks/${t.id}/send`))}>发送确认卡片</button>
            </div>}
          </div>
          {detail.deliveries.filter(d => d.task_id === t.id).map((d, i) => <p key={i} className="text-sm text-gray-500">{d.chat_name}（{d.chat_id}）：{d.withdrawn ? '消息已撤回，请点击发送确认卡片重新发送' : d.sent ? '已发送' : '发送失败或待重试'} · 版本 {d.task_version}</p>)}
          <IndividualResponses responses={(detail.responses || []).filter(r => r.task_id === t.id)} dimensions={dimensions} open={detail.status === 'open'} run={run} refresh={refresh} />
        </div>)}
        <h3 className="font-medium">处置与关闭记录</h3>
        {detail.admin_events.map((e, i) => <p key={i}>{localDateTime(e.created_at)} · 管理员 #{e.admin_user_id} · {TASK_STATUSES[e.old_status] || e.old_status} → {TASK_STATUSES[e.status] || e.status}：{e.details}</p>)}
        {detail.status === 'open' && <div className="space-y-1">
          <p className="text-xs text-gray-500">三个维度（P 产线 / NovoOne 产线 / 运维）全部完成回执后才能关闭；关闭后不能再提交或修改回执。</p>
          <div className="flex gap-2"><input className={input} placeholder="填写关闭说明" value={closeNote} onChange={e => setCloseNote(e.target.value)} /><button className={button} disabled={busy || !closeNote.trim()} onClick={() => run(async () => { await api.post(`/receipts/cases/${caseId}/close`, { details: closeNote }); setCloseNote('') })}>关闭确认单</button></div>
        </div>}
        <button className="text-indigo-700" onClick={() => run(() => Promise.resolve())}>刷新回执</button> <button className="text-indigo-700" disabled={busy} onClick={exportCsv}>导出回执 CSV</button>
      </>}
    </section>
  </div>
}
