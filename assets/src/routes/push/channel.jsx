import { useState, useEffect } from 'react'
import {
    listPushChannels,
    createPushChannel,
    updatePushChannel,
    deletePushChannel,
    testPushChannel,
} from '../../lib/api'

const CHANNEL_TYPES = [
    { value: 'dingtalk', label: '钉钉机器人' },
    { value: 'wecom', label: '企业微信机器人' },
    { value: 'feishu', label: '飞书机器人' },
]

const emptyForm = {
    name: '',
    channel_type: 'dingtalk',
    access_token: '',
    secret_token: '',
    status: true,
}

const PushChannelPage = () => {
    const [channels, setChannels] = useState([])
    const [loading, setLoading] = useState(true)
    const [saving, setSaving] = useState(false)
    const [editingId, setEditingId] = useState(null)
    const [testingId, setTestingId] = useState(null)
    const [form, setForm] = useState(emptyForm)
    const [error, setError] = useState('')
    const [success, setSuccess] = useState('')

    useEffect(() => {
        fetchChannels()
    }, [])

    const fetchChannels = async () => {
        setLoading(true)
        setError('')
        try {
            const response = await listPushChannels()
            setChannels(response.data.data || [])
        } catch (err) {
            setError('获取推送渠道失败')
            console.error('Fetch push channels error:', err)
        } finally {
            setLoading(false)
        }
    }

    const handleChange = (e) => {
        const { name, value, type, checked } = e.target
        setForm((prev) => ({
            ...prev,
            [name]: type === 'checkbox' ? checked : value,
        }))
    }

    const handleEdit = (ch) => {
        setEditingId(ch.id)
        setForm({
            name: ch.name,
            channel_type: ch.channel_type,
            access_token: ch.access_token,
            secret_token: ch.secret_token || '',
            status: ch.status,
        })
        setSuccess('')
        setError('')
    }

    const handleCancel = () => {
        setEditingId(null)
        setForm(emptyForm)
    }

    const handleSubmit = async (e) => {
        e.preventDefault()
        setSaving(true)
        setError('')
        setSuccess('')
        try {
            if (editingId) {
                await updatePushChannel(editingId, form)
                setSuccess('推送渠道已更新')
            } else {
                await createPushChannel(form)
                setSuccess('推送渠道已创建')
            }
            handleCancel()
            fetchChannels()
        } catch (err) {
            setError(err.response?.data?.data?.message || '保存推送渠道失败')
            console.error('Save push channel error:', err)
        } finally {
            setSaving(false)
        }
    }

    const handleDelete = async (ch) => {
        if (!window.confirm(`确定删除渠道「${ch.name}」？`)) return
        setError('')
        setSuccess('')
        try {
            await deletePushChannel(ch.id)
            setSuccess('推送渠道已删除')
            fetchChannels()
        } catch (err) {
            setError('删除推送渠道失败')
            console.error('Delete push channel error:', err)
        }
    }

    const handleTest = async (ch) => {
        setTestingId(ch.id)
        setError('')
        setSuccess('')
        try {
            await testPushChannel(ch.id)
            setSuccess(`测试消息已发送到「${ch.name}」`)
        } catch (err) {
            setError(err.response?.data?.data?.message || `测试「${ch.name}」失败`)
        } finally {
            setTestingId(null)
        }
    }

    const typeLabel = (t) =>
        CHANNEL_TYPES.find((x) => x.value === t)?.label || t

    const inputCls =
        'block w-full px-3 py-2 placeholder-gray-400 border border-gray-300 rounded-md shadow-sm appearance-none focus:outline-none focus:ring-indigo-500 focus:border-indigo-500 sm:text-sm'
    const btnPrimary =
        'inline-flex justify-center px-4 py-2 text-sm font-medium text-white bg-indigo-600 border border-transparent rounded-md shadow-sm hover:bg-indigo-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500 disabled:opacity-50'
    const btnGhost =
        'inline-flex justify-center px-3 py-2 text-sm font-medium text-gray-700 bg-white border border-gray-300 rounded-md shadow-sm hover:bg-gray-50 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500 disabled:opacity-50'
    const btnDanger =
        'inline-flex justify-center px-3 py-2 text-sm font-medium text-red-700 bg-white border border-red-300 rounded-md shadow-sm hover:bg-red-50 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-red-500 disabled:opacity-50'

    return (
        <div className="max-w-5xl mx-auto">
            <div className="mb-8 text-center">
                <h1 className="mb-2 text-3xl font-extrabold text-gray-900">推送渠道</h1>
                <p className="text-gray-600">配置多个 IM 机器人接收漏洞与公告推送（钉钉/企微/飞书）</p>
            </div>

            {error && (
                <div className="p-4 mb-6 border border-red-200 rounded-md bg-red-50">
                    <div className="text-sm text-red-800">{error}</div>
                </div>
            )}
            {success && (
                <div className="p-4 mb-6 border border-green-200 rounded-md bg-green-50">
                    <div className="text-sm text-green-800">{success}</div>
                </div>
            )}

            {loading ? (
                <div className="flex items-center justify-center h-64">
                    <div className="w-12 h-12 border-b-2 border-indigo-600 rounded-full animate-spin" />
                </div>
            ) : (
                <>
                    <div className="overflow-hidden bg-white shadow sm:rounded-lg mb-8">
                        <div className="px-4 py-5 border-b border-gray-200 sm:px-6">
                            <h2 className="text-lg font-medium leading-6 text-gray-900">已配置渠道</h2>
                        </div>
                        {channels.length === 0 ? (
                            <div className="px-4 py-8 text-center text-gray-500 sm:px-6">
                                暂无渠道，请在下方添加
                            </div>
                        ) : (
                            <ul className="divide-y divide-gray-200">
                                {channels.map((ch) => (
                                    <li
                                        key={ch.id}
                                        className="flex items-center justify-between px-4 py-4 sm:px-6"
                                    >
                                        <div className="min-w-0 mr-4">
                                            <p className="text-sm font-medium text-gray-900">
                                                {ch.name}
                                                <span className="ml-2 text-xs text-gray-500">
                                                    {typeLabel(ch.channel_type)}
                                                </span>
                                                {!ch.status && (
                                                    <span className="ml-2 text-xs text-red-500">已停用</span>
                                                )}
                                            </p>
                                            <p className="text-xs text-gray-400 truncate max-w-md">
                                                {ch.access_token}
                                            </p>
                                        </div>
                                        <div className="flex flex-shrink-0 space-x-2">
                                            <button type="button" onClick={() => handleEdit(ch)} className={btnGhost}>
                                                编辑
                                            </button>
                                            <button
                                                type="button"
                                                onClick={() => handleTest(ch)}
                                                disabled={testingId === ch.id}
                                                className={btnGhost}
                                            >
                                                {testingId === ch.id ? '发送中...' : '测试'}
                                            </button>
                                            <button type="button" onClick={() => handleDelete(ch)} className={btnDanger}>
                                                删除
                                            </button>
                                        </div>
                                    </li>
                                ))}
                            </ul>
                        )}
                    </div>

                    <div className="overflow-hidden bg-white shadow sm:rounded-lg">
                        <div className="px-4 py-5 border-b border-gray-200 sm:px-6">
                            <h2 className="text-lg font-medium leading-6 text-gray-900">
                                {editingId ? '编辑渠道' : '添加渠道'}
                            </h2>
                        </div>
                        <form onSubmit={handleSubmit} className="px-4 py-5 sm:px-6">
                            <div className="grid grid-cols-1 gap-y-6 gap-x-4 sm:grid-cols-6">
                                <div className="sm:col-span-3">
                                    <label htmlFor="name" className="block text-sm font-medium text-gray-700">
                                        渠道名称
                                    </label>
                                    <div className="mt-1">
                                        <input
                                            type="text"
                                            name="name"
                                            id="name"
                                            value={form.name}
                                            onChange={handleChange}
                                            required
                                            className={inputCls}
                                        />
                                    </div>
                                </div>
                                <div className="sm:col-span-3">
                                    <label htmlFor="channel_type" className="block text-sm font-medium text-gray-700">
                                        渠道类型
                                    </label>
                                    <div className="mt-1">
                                        <select
                                            name="channel_type"
                                            id="channel_type"
                                            value={form.channel_type}
                                            onChange={handleChange}
                                            className={inputCls}
                                        >
                                            {CHANNEL_TYPES.map((t) => (
                                                <option key={t.value} value={t.value}>
                                                    {t.label}
                                                </option>
                                            ))}
                                        </select>
                                    </div>
                                </div>
                                <div className="sm:col-span-4">
                                    <label htmlFor="access_token" className="block text-sm font-medium text-gray-700">
                                        令牌（Access Token / Webhook Key）
                                    </label>
                                    <div className="mt-1">
                                        <input
                                            type="text"
                                            name="access_token"
                                            id="access_token"
                                            value={form.access_token}
                                            onChange={handleChange}
                                            required
                                            className={inputCls}
                                        />
                                    </div>
                                    <p className="mt-2 text-sm text-gray-500">
                                        钉钉:机器人 Access Token;企微:Webhook Key;飞书:Webhook Token
                                    </p>
                                </div>
                                <div className="sm:col-span-4">
                                    <label htmlFor="secret_token" className="block text-sm font-medium text-gray-700">
                                        加签密钥（可选）
                                    </label>
                                    <div className="mt-1">
                                        <input
                                            type="text"
                                            name="secret_token"
                                            id="secret_token"
                                            value={form.secret_token}
                                            onChange={handleChange}
                                            placeholder="钉钉/飞书加签密钥,企微留空"
                                            className={inputCls}
                                        />
                                    </div>
                                </div>
                                <div className="flex items-center sm:col-span-6">
                                    <input
                                        id="status"
                                        name="status"
                                        type="checkbox"
                                        checked={form.status}
                                        onChange={handleChange}
                                        className="w-4 h-4 text-indigo-600 border-gray-300 rounded focus:ring-indigo-500"
                                    />
                                    <label htmlFor="status" className="block ml-2 text-sm text-gray-700">
                                        启用该渠道
                                    </label>
                                </div>
                            </div>
                            <div className="pt-5 mt-6 border-t border-gray-200">
                                <button type="submit" disabled={saving} className={btnPrimary}>
                                    {saving ? '保存中...' : editingId ? '更新渠道' : '添加渠道'}
                                </button>
                                {editingId && (
                                    <button type="button" onClick={handleCancel} className={`ml-3 ${btnGhost}`}>
                                        取消编辑
                                    </button>
                                )}
                            </div>
                        </form>
                    </div>
                </>
            )}
        </div>
    )
}

export default PushChannelPage
