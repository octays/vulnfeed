import { useEffect, useRef, useState } from 'react'
import api, { getSyncDataTask, createOrUpdateSyncDataTask, runSyncDataTaskNow } from '../../lib/api'
import FeishuDirectory from '../feishu-directory'

const input = 'block w-full px-3 py-2 border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-indigo-500 focus:border-indigo-500 sm:text-sm'
const primary = 'inline-flex justify-center px-4 py-2 text-sm font-medium text-white bg-indigo-600 border border-transparent rounded-md shadow-sm hover:bg-indigo-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500 disabled:opacity-50'

function Section({ title, description, children, actions }) {
  return (
    <section className="overflow-hidden bg-white shadow sm:rounded-lg">
      <div className="flex items-center justify-between px-4 py-4 border-b border-gray-200 sm:px-6">
        <div>
          <h2 className="text-base font-semibold leading-6 text-gray-900">{title}</h2>
          {description && <p className="mt-0.5 text-xs text-gray-500">{description}</p>}
        </div>
        {actions}
      </div>
      <div className="px-4 py-5 sm:px-6">{children}</div>
    </section>
  )
}

function Toggle({ id, label, hint, checked, onChange }) {
  return (
    <label htmlFor={id} className="flex items-start gap-3 p-3 rounded-lg cursor-pointer hover:bg-gray-50">
      <div className="flex items-center h-5 mt-0.5">
        <input id={id} name={id} type="checkbox" checked={checked} onChange={onChange} className="w-4 h-4 text-indigo-600 border-gray-300 rounded focus:ring-indigo-500" />
      </div>
      <div className="text-sm">
        <span className="font-medium text-gray-700">{label}</span>
        <p className="text-gray-500">{hint}</p>
      </div>
    </label>
  )
}

const SyncDataTaskPage = () => {
  const [task, setTask] = useState(null)
  const [saved, setSaved] = useState(null)
  const [people, setPeople] = useState([])
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [running, setRunning] = useState(false)
  const [error, setError] = useState('')
  const [success, setSuccess] = useState('')
  const successTimer = useRef(null)

  const [members, setMembers] = useState([])

  useEffect(() => {
    fetchSyncDataTask()
    api.get('/receipts/people').then(r => setPeople(r.data.data || [])).catch(() => {})
    api.get('/receipts/directory/members').then(r => setMembers(r.data.data || [])).catch(() => {})
    return () => clearTimeout(successTimer.current)
  }, [])

  const flash = (message) => {
    setSuccess(message)
    clearTimeout(successTimer.current)
    successTimer.current = setTimeout(() => setSuccess(''), 3000)
  }

  const fetchSyncDataTask = async () => {
    setLoading(true)
    setError('')
    try {
      const response = await getSyncDataTask()
      const { data } = response.data
      if (data) {
        setSaved({
          name: data.name,
          interval_minutes: data.interval_minutes,
          status: data.status,
          skip_tls_verify: data.skip_tls_verify,
          push_history: data.push_history,
          auto_receipts: data.auto_receipts,
          auto_remind: !!data.auto_remind,
          allow_response_deletion: !!data.allow_response_deletion,
          remind_escalate_open_id: data.remind_escalate_open_id || '',
        })
        setTask({
          name: data.name,
          interval_minutes: data.interval_minutes,
          status: data.status,
          skip_tls_verify: data.skip_tls_verify,
          push_history: data.push_history,
          auto_receipts: data.auto_receipts,
          auto_remind: !!data.auto_remind,
          allow_response_deletion: !!data.allow_response_deletion,
          remind_escalate_open_id: data.remind_escalate_open_id || '',
        })
      }
    } catch (err) {
      setError('获取同步任务信息失败')
      console.error('Fetch sync data task error:', err)
    } finally {
      setLoading(false)
    }
  }

  const dirty = !!task && !!saved && JSON.stringify(task) !== JSON.stringify(saved)

  const handleChange = (e) => {
    const { name, value, type, checked } = e.target
    setTask(prev => ({
      ...prev,
      [name]: type === 'checkbox' ? checked : type === 'number' ? parseInt(value) : value,
    }))
  }

  const handleSubmit = async (e) => {
    e.preventDefault()
    setSaving(true)
    setError('')
    try {
      await createOrUpdateSyncDataTask(task)
      setSaved({ ...task })
      flash('配置已保存')
    } catch (err) {
      setError(err.response?.data?.data?.message || err.response?.data?.message || '保存同步任务配置失败')
      console.error('Save sync data task error:', err)
    } finally {
      setSaving(false)
    }
  }

  const handleRunNow = async () => {
    setRunning(true)
    setError('')
    try {
      await runSyncDataTaskNow()
      flash('已触发立即同步，后台运行中')
    } catch (err) {
      setError(err.response?.data?.data?.message || '触发立即同步失败')
      console.error('Run sync task now error:', err)
    } finally {
      setRunning(false)
    }
  }

  const escalateOpenId = (task?.remind_escalate_open_id || '').trim()
  const contacts = Object.values(
    [...people.map(p => ({ open_id: p.open_id, name: p.name, department: '' })), ...members]
      .reverse()
      .reduce((acc, c) => { if (c.open_id) acc[c.open_id] = c; return acc }, {}),
  )
  const escalateName = escalateOpenId ? contacts.find(p => p.open_id === escalateOpenId)?.name : null

  return (
    <div className="max-w-4xl mx-auto space-y-6">
      <div className="text-center">
        <h1 className="mb-1 text-2xl font-extrabold text-gray-900">数据同步任务配置</h1>
        <p className="text-sm text-gray-600">调度、自动确认与催办、回执接收的统一配置</p>
      </div>

      {error && (
        <div className="p-3 border border-red-200 rounded-md bg-red-50 text-sm text-red-800 flex justify-between items-center">
          <span>{error}</span>
          <button className="text-red-600" onClick={() => setError('')}>✕</button>
        </div>
      )}
      {success && (
        <div className="p-3 border border-green-200 rounded-md bg-green-50 text-sm text-green-800">{success}</div>
      )}

      {loading || !task ? (
        <div className="flex items-center justify-center h-64">
          <div className="w-12 h-12 border-b-2 border-indigo-600 rounded-full animate-spin"></div>
        </div>
      ) : (
        <>
          <Section
            title="基础调度"
            description="采集频率与任务启停"
            actions={<button type="button" onClick={handleRunNow} disabled={running} className="px-3 py-1.5 text-sm font-medium text-indigo-700 border border-indigo-300 rounded-md hover:bg-indigo-50 disabled:opacity-50">{running ? '触发中…' : '▶ 立即同步'}</button>}
          >
            <div className="grid gap-4 sm:grid-cols-2">
              <div>
                <label htmlFor="name" className="block text-sm font-medium text-gray-700">任务名称</label>
                <input type="text" name="name" id="name" value={task.name} onChange={handleChange} required className={`mt-1 ${input}`} />
              </div>
              <div>
                <label htmlFor="interval_minutes" className="block text-sm font-medium text-gray-700">同步间隔（分钟）</label>
                <input type="number" name="interval_minutes" id="interval_minutes" min="1" max="1440" value={task.interval_minutes} onChange={handleChange} required className={`mt-1 ${input}`} />
                <p className="mt-1 text-xs text-gray-500">1–1440 分钟（最长 1 天）</p>
              </div>
            </div>
            <div className="mt-2 -mx-3">
              <Toggle id="status" label="启用任务" hint="启用后按设定间隔自动同步数据" checked={task.status} onChange={handleChange} />
            </div>
          </Section>

          <form onSubmit={handleSubmit}>
            <Section title="自动确认与催办" description="确认单的发起时机与催办策略">
              <div className="-mx-3 space-y-1">
                <Toggle id="auto_receipts" label="自动发起影响确认" hint="新入库且披露日期在 48 小时内（昨天及以后）的数据自动创建确认单并发送飞书卡片；历史数据不自动发起。" checked={task.auto_receipts} onChange={handleChange} />
                <Toggle id="auto_remind" label="自动催办未回执维度" hint="每 24 小时私发催办一次给未回执维度的负责人，次数不设上限；第 3 次起追加通知下方升级提醒人。" checked={task.auto_remind} onChange={handleChange} />
                <Toggle id="allow_response_deletion" label="允许填写人删除自己的回执" hint="开启后确认卡片显示「删除我的回执」按钮（软删除并保留审计）；关闭则只能再次提交覆盖。" checked={task.allow_response_deletion} onChange={handleChange} />
              </div>
              <div className="mt-3 p-3 rounded-lg border border-gray-200 bg-gray-50/60">
                <label htmlFor="remind_escalate_open_id" className="block text-sm font-medium text-gray-700">催办升级提醒人</label>
                <input id="remind_escalate_open_id" name="remind_escalate_open_id" list="sync-people" type="text" value={task.remind_escalate_open_id} onChange={handleChange} className={`mt-1 ${input}`} placeholder="从右侧选择或输入 ou_…（第 3 次催办起通知，留空不升级）" />
                <datalist id="sync-people">
                  {contacts.map(p => <option key={p.open_id} value={p.open_id}>{p.department ? `${p.name} · ${p.department}` : p.name}</option>)}
                </datalist>
                <p className="mt-1 text-xs text-gray-500">
                  {escalateName ? <>当前：<b>{escalateName}</b>（{task.remind_escalate_open_id}）</> : '输入框可从提交过回执的成员中选，也可直接粘贴任意 ou_ 开头的 Open ID'}
                </p>
              </div>
            </Section>

            <Section title="推送与安全" description="历史数据与证书校验">
              <div className="-mx-3 space-y-1">
                <Toggle id="push_history" label="允许自动推送历史数据" hint="关闭时只推送发布日期为今天及以后的数据；开启后同步遇到的历史数据也自动推送。" checked={task.push_history} onChange={handleChange} />
                <Toggle id="skip_tls_verify" label="跳过同步源的证书校验" hint="仅在信任同步源及网络环境时启用。" checked={task.skip_tls_verify} onChange={handleChange} />
              </div>
              <div className="flex items-center justify-between pt-4 mt-4 border-t border-gray-200">
                <span className={`text-sm ${dirty ? 'text-amber-700' : 'text-gray-400'}`}>{dirty ? '● 有未保存的修改' : '配置无变更'}</span>
                <div className="flex gap-2">
                  {dirty && <button type="button" onClick={() => setTask({ ...saved })} className="px-4 py-2 text-sm font-medium text-gray-700 border border-gray-300 rounded-md hover:bg-gray-50">放弃修改</button>}
                  <button type="submit" disabled={saving || !dirty} className={primary}>{saving ? '保存中…' : '保存配置'}</button>
                </div>
              </div>
            </Section>
          </form>

          <Section title="影响确认接收设置" description="确认卡片发送到启用的接收群；姓名与部门由飞书通讯录自动解析">
            <FeishuDirectory />
          </Section>
        </>
      )}
    </div>
  )
}

export default SyncDataTaskPage
