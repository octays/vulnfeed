import { useState, useEffect } from 'react'
import api, { getSyncDataTask, createOrUpdateSyncDataTask, runSyncDataTaskNow } from '../../lib/api'
import FeishuDirectory from '../feishu-directory'

const SyncDataTaskPage = () => {
  const [task, setTask] = useState({
    name: '',
    interval_minutes: 60,
    status: true,
    skip_tls_verify: false,
    push_history: false,
    auto_receipts: false,
    auto_remind: false,
    allow_response_deletion: false,
    remind_escalate_open_id: '',
  })
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
    const [running, setRunning] = useState(false)
  const [error, setError] = useState('')
  const [success, setSuccess] = useState('')

  useEffect(() => {
    fetchSyncDataTask()
  }, [])

  const fetchSyncDataTask = async () => {
    setLoading(true)
    setError('')

    try {
      const response = await getSyncDataTask()
      const { data } = response.data
      if (data) {
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
    setSuccess('')

    try {
      await createOrUpdateSyncDataTask(task)
      setSuccess('同步任务配置已保存')
      // 重新获取任务信息以确保数据是最新的
      fetchSyncDataTask()
    } catch (err) {
      setError('保存同步任务配置失败')
      console.error('Save sync data task error:', err)
    } finally {
      setSaving(false)
    }
  }

    const handleRunNow = async () => {
        setRunning(true)
        setError('')
        setSuccess('')
        try {
            await runSyncDataTaskNow()
            setSuccess('已触发立即同步,后台运行中')
        } catch (err) {
            setError('触发立即同步失败')
            console.error('Run sync task now error:', err)
        } finally {
            setRunning(false)
        }
    }

  return (
    <div>
      <div className="max-w-3xl mx-auto">
      <div className="mb-8 text-center">
        <h1 className="mb-2 text-3xl font-extrabold text-gray-900">
          数据同步任务配置
        </h1>
        <p className="text-gray-600">
          配置和管理漏洞数据同步任务
        </p>
      </div>

      {error && (
        <div className="p-4 mb-6 border border-red-200 rounded-md bg-red-50">
          <div className="text-sm text-red-800">
            {error}
          </div>
        </div>
      )}

      {success && (
        <div className="p-4 mb-6 border border-green-200 rounded-md bg-green-50">
          <div className="text-sm text-green-800">
            {success}
          </div>
        </div>
      )}

      {loading ? (
        <div className="flex items-center justify-center h-64">
          <div className="w-12 h-12 border-b-2 border-indigo-600 rounded-full animate-spin"></div>
        </div>
      ) : (
        <div className="overflow-hidden bg-white shadow sm:rounded-lg">
<div className="flex items-center justify-between px-4 py-5 border-b border-gray-200 sm:px-6">
            <h2 className="text-lg font-medium leading-6 text-gray-900">
              同步任务设置
            </h2>
                    <button
                        type="button"
                        onClick={handleRunNow}
                        disabled={running}
                        className="inline-flex justify-center px-4 py-2 text-sm font-medium text-white bg-indigo-600 border border-transparent rounded-md shadow-sm hover:bg-indigo-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500 disabled:opacity-50"
                    >
                        {running ? '触发中...' : '立即运行'}
                    </button>
          </div>
          <form onSubmit={handleSubmit} className="px-4 py-5 sm:px-6">
            <div className="grid grid-cols-1 gap-y-6 gap-x-4 sm:grid-cols-6">
              <div className="sm:col-span-4">
                <label htmlFor="name" className="block text-sm font-medium text-gray-700">
                  任务名称
                </label>
                <div className="mt-1">
                  <input
                    type="text"
                    name="name"
                    id="name"
                    value={task.name}
                    onChange={handleChange}
                    required
                    className="block w-full px-3 py-2 placeholder-gray-400 border border-gray-300 rounded-md shadow-sm appearance-none focus:outline-none focus:ring-indigo-500 focus:border-indigo-500 sm:text-sm"
                  />
                </div>
              </div>

              <div className="sm:col-span-4">
                <label htmlFor="interval_minutes" className="block text-sm font-medium text-gray-700">
                  同步间隔（分钟）
                </label>
                <div className="mt-1">
                  <input
                    type="number"
                    name="interval_minutes"
                    id="interval_minutes"
                    min="1"
                    max="1440"
                    value={task.interval_minutes}
                    onChange={handleChange}
                    required
                    className="block w-full px-3 py-2 placeholder-gray-400 border border-gray-300 rounded-md shadow-sm appearance-none focus:outline-none focus:ring-indigo-500 focus:border-indigo-500 sm:text-sm"
                  />
                </div>
                <p className="mt-2 text-sm text-gray-500">
                  设置数据同步的时间间隔，范围1-1440分钟（1天）
                </p>
              </div>

              <div className="sm:col-span-4">
                <div className="flex items-start">
                  <div className="flex items-center h-5"><input id="auto_receipts" name="auto_receipts" type="checkbox" checked={task.auto_receipts} onChange={handleChange} className="w-4 h-4 text-indigo-600 border-gray-300 rounded focus:ring-indigo-500" /></div>
                  <div className="ml-3 text-sm"><label htmlFor="auto_receipts" className="font-medium text-gray-700">自动发起影响确认</label><p className="text-gray-500">新入库且披露日期在 48 小时内（昨天及以后）的同步数据自动创建确认单并发送飞书卡片。历史或日期无法识别的数据不会自动发起。</p></div>
                </div>
              </div>

              <div className="sm:col-span-4">
                <div className="flex items-start">
                  <div className="flex items-center h-5"><input id="auto_remind" name="auto_remind" type="checkbox" checked={task.auto_remind} onChange={handleChange} className="w-4 h-4 text-indigo-600 border-gray-300 rounded focus:ring-indigo-500" /></div>
                  <div className="ml-3 text-sm"><label htmlFor="auto_remind" className="font-medium text-gray-700">自动催办未回执维度</label><p className="text-gray-500">确认单仍有维度未回执时，每 24 小时自动催办一次并 @ 该维度负责人，次数不设上限；第 3 次起追加 @ 下方升级提醒人。</p></div>
                </div>
              </div>

              <div className="sm:col-span-4">
                <label htmlFor="remind_escalate_open_id" className="block text-sm font-medium text-gray-700">催办升级提醒人 Open ID</label>
                <input id="remind_escalate_open_id" name="remind_escalate_open_id" type="text" value={task.remind_escalate_open_id} onChange={handleChange} className="mt-1 px-3 py-2 border rounded w-full" placeholder="ou_…（第 3 次催办起追加 @，留空不升级）" />
              </div>

              <div className="sm:col-span-4">
                <div className="flex items-start">
                  <div className="flex items-center h-5"><input id="allow_response_deletion" name="allow_response_deletion" type="checkbox" checked={task.allow_response_deletion} onChange={handleChange} className="w-4 h-4 text-indigo-600 border-gray-300 rounded focus:ring-indigo-500" /></div>
                  <div className="ml-3 text-sm"><label htmlFor="allow_response_deletion" className="font-medium text-gray-700">允许填写人删除自己的回执</label><p className="text-gray-500">开启后确认卡片上显示「删除我的回执」按钮，填写人可撤回自己的提交（软删除并保留审计）；关闭则只能通过再次提交覆盖变更。对所有确认单统一生效。</p></div>
                </div>
              </div>

              <div className="sm:col-span-4">
                <div className="flex items-start">
                  <div className="flex items-center h-5">
                    <input id="push_history" name="push_history" type="checkbox" checked={task.push_history} onChange={handleChange} className="w-4 h-4 text-indigo-600 border-gray-300 rounded focus:ring-indigo-500" />
                  </div>
                  <div className="ml-3 text-sm">
                    <label htmlFor="push_history" className="font-medium text-gray-700">允许自动推送历史数据</label>
                    <p className="text-gray-500">关闭时只推送发布日期为今天或以后的数据，日期无法识别的数据不会自动推送。开启后允许推送同步时遇到的历史数据。</p>
                  </div>
                </div>
              </div>

              <div className="sm:col-span-4">
                <div className="flex items-start">
                  <div className="flex items-center h-5">
                    <input id="skip_tls_verify" name="skip_tls_verify" type="checkbox" checked={task.skip_tls_verify} onChange={handleChange} className="w-4 h-4 text-indigo-600 border-gray-300 rounded focus:ring-indigo-500" />
                  </div>
                  <div className="ml-3 text-sm">
                    <label htmlFor="skip_tls_verify" className="font-medium text-gray-700">跳过同步源的证书校验</label>
                    <p className="text-gray-500">仅在信任同步源及网络环境时启用；无法验证服务器身份。</p>
                  </div>
                </div>
              </div>

              <div className="sm:col-span-4">
                <div className="flex items-start">
                  <div className="flex items-center h-5">
                    <input
                      id="status"
                      name="status"
                      type="checkbox"
                      checked={task.status}
                      onChange={handleChange}
                      className="w-4 h-4 text-indigo-600 border-gray-300 rounded focus:ring-indigo-500"
                    />
                  </div>
                  <div className="ml-3 text-sm">
                    <label htmlFor="status" className="font-medium text-gray-700">
                      启用任务
                    </label>
                    <p className="text-gray-500">
                      启用后将按照设定的时间间隔自动同步数据
                    </p>
                  </div>
                </div>
              </div>
            </div>

            <div className="pt-5 mt-8 border-t border-gray-200">
              <div className="flex justify-end">
                <button
                  type="submit"
                  disabled={saving}
                  className="inline-flex justify-center px-4 py-2 ml-3 text-sm font-medium text-white bg-indigo-600 border border-transparent rounded-md shadow-sm hover:bg-indigo-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500 disabled:opacity-50"
                >
                  {saving ? '保存中...' : '保存配置'}
                </button>
              </div>
            </div>
          </form>
        </div>
      )}
      </div>

      <div className="w-[80%] mx-auto my-8">
        <div className="p-5 bg-white rounded shadow">
          <h3 className="text-lg font-medium mb-3">影响确认接收设置</h3>
          <p className="mb-4 text-sm text-gray-500">确认卡片发送到下方启用的接收群；填写人的团队与姓名由飞书部门/通讯录自动解析。</p>
          <FeishuDirectory />
        </div>
      </div>
    </div>
  )
}

export default SyncDataTaskPage
