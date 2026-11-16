import { useState } from 'react'
import { triggerReceipt } from '../lib/api'

export default function TriggerReceiptButton({ sourceType, sourceId, label = '发起确认', onComplete }) {
  const [busy, setBusy] = useState(false)
  const [result, setResult] = useState(null)

  async function trigger(event) {
    event.preventDefault()
    event.stopPropagation()
    setBusy(true)
    setResult(null)
    try {
      const { data } = await triggerReceipt(sourceType, sourceId)
      if (data.failures.length) {
        const details = data.failures.map(f => `${f.team}：${f.message}`).join('；')
        setResult({ error: true, text: `确认单 #${data.case_id}：${details}。其他团队可能已发送，请检查确认单后重试。` })
      } else if (!data.sent_teams.length) {
        setResult({ error: false, text: `确认单 #${data.case_id} 没有待确认的团队。` })
      } else {
        setResult({ error: false, text: `确认单 #${data.case_id} 已发送或此前已发送至 ${data.sent_teams.join('、')}。` })
      }
      if (onComplete) { try { await onComplete() } catch (_) { /* 发送结果仍有效，刷新失败可手动重试 */ } }
    } catch (error) {
      setResult({ error: true, text: error.response?.data?.data?.message || '发起确认失败，请检查接收设置' })
    } finally {
      setBusy(false)
    }
  }

  return <>
    <button type="button" onClick={trigger} disabled={busy}
      className="ml-2 inline-flex items-center px-2.5 py-0.5 border border-emerald-300 rounded-full text-xs font-medium text-emerald-700 bg-white hover:bg-emerald-50 disabled:opacity-50">
      {busy ? '发送中' : label}
    </button>
    {result && <span role="status" className={`ml-2 text-xs ${result.error ? 'text-red-700' : 'text-green-700'}`}>{result.text}</span>}
  </>
}
