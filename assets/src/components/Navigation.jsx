import { Link, useLocation } from 'react-router-dom'
import { changePassword } from '../lib/api'
import { useState, useRef, useEffect } from 'react'

const Navigation = () => {
  const location = useLocation()
  const [userMenuOpen, setUserMenuOpen] = useState(false)
  const userMenuRef = useRef(null)
const [showChangePwd, setShowChangePwd] = useState(false)
const [oldPassword, setOldPassword] = useState('')
const [newPassword, setNewPassword] = useState('')
const [confirmPassword, setConfirmPassword] = useState('')
const [pwdError, setPwdError] = useState('')
const [pwdSubmitting, setPwdSubmitting] = useState(false)

  // 检查用户是否已登录
  const [isAuthenticated, setIsAuthenticated] = useState(false)

  useEffect(() => {
    const token = localStorage.getItem('token')
    setIsAuthenticated(!!token)
  }, [])

  const isActive = (path) => {
    return location.pathname === path
  }

  const handleLogout = () => {
    // 清除token
    localStorage.removeItem('token')
    // 重定向到登录页面
    window.location.href = '/login'
  }

  const openChangePwd = () => {
    setUserMenuOpen(false)
    setShowChangePwd(true)
    setOldPassword('')
    setNewPassword('')
    setConfirmPassword('')
    setPwdError('')
}

const handleChangePassword = async (e) => {
    e.preventDefault()
    if (newPassword !== confirmPassword) {
        setPwdError('两次输入的新密码不一致')
        return
    }
    setPwdSubmitting(true)
    setPwdError('')
    try {
        await changePassword(oldPassword, newPassword)
        setShowChangePwd(false)
        window.alert('密码已修改')
    } catch (err) {
        setPwdError(err.response?.data?.data?.message || '修改密码失败')
    } finally {
        setPwdSubmitting(false)
    }
}

// 点击外部关闭用户菜单
  useEffect(() => {
    const handleClickOutside = (event) => {
      if (userMenuRef.current && !userMenuRef.current.contains(event.target)) {
        setUserMenuOpen(false)
      }
    }

    document.addEventListener('mousedown', handleClickOutside)
    return () => {
      document.removeEventListener('mousedown', handleClickOutside)
    }
  }, [])

  return (
    <nav className="bg-white shadow">
      <div className="px-4 mx-auto max-w-7xl sm:px-6 lg:px-8">
        <div className="flex justify-between h-16">
          <div className="flex">
            <div className="flex items-center flex-shrink-0">
              <span className="text-xl font-bold text-indigo-600">VulnFeed</span>
            </div>
            <div className="hidden sm:ml-6 sm:flex sm:space-x-8">
              <Link
                to="/vulns"
                className={`${
                  isActive('/vulns') || location.pathname === '/'
                    ? 'border-indigo-500 text-gray-900'
                    : 'border-transparent text-gray-500 hover:border-gray-300 hover:text-gray-700'
                } inline-flex items-center px-1 pt-1 border-b-2 text-sm font-medium`}
              >
                漏洞列表
              </Link>
              <Link
                to="/secnotice"
                className={`${
                  isActive('/secnotice')
                    ? 'border-indigo-500 text-gray-900'
                    : 'border-transparent text-gray-500 hover:border-gray-300 hover:text-gray-700'
                } inline-flex items-center px-1 pt-1 border-b-2 text-sm font-medium`}
              >
                安全公告
              </Link>
              <Link
                to="/plugins"
                className={`${
                  isActive('/plugins')
                    ? 'border-indigo-500 text-gray-900'
                    : 'border-transparent text-gray-500 hover:border-gray-300 hover:text-gray-700'
                } inline-flex items-center px-1 pt-1 border-b-2 text-sm font-medium`}
              >
                插件管理
              </Link>
              {isAuthenticated && (
                <Link
                  to="/sync/task"
                  className={`${
                    isActive('/sync/task')
                      ? 'border-indigo-500 text-gray-900'
                      : 'border-transparent text-gray-500 hover:border-gray-300 hover:text-gray-700'
                  } inline-flex items-center px-1 pt-1 border-b-2 text-sm font-medium`}
                >
                  同步任务
                </Link>
              )}
              {isAuthenticated && (
                <Link
                                        to="/push/channels"
                  className={`${
                                        isActive('/push/channels')
                      ? 'border-indigo-500 text-gray-900'
                      : 'border-transparent text-gray-500 hover:border-gray-300 hover:text-gray-700'
                  } inline-flex items-center px-1 pt-1 border-b-2 text-sm font-medium`}
                >
                                        推送渠道
                </Link>
              )}
            </div>
          </div>
          {isAuthenticated ? (
            <div className="hidden sm:ml-6 sm:flex sm:items-center">
              <div className="relative ml-3">
                <div>
                  <button
                    onClick={() => setUserMenuOpen(!userMenuOpen)}
                    className="flex text-sm rounded-full focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500"
                  >
                    <span className="sr-only">打开用户菜单</span>
                    <div className="flex items-center justify-center w-8 h-8 bg-indigo-100 rounded-full">
                      <span className="font-medium text-indigo-800">U</span>
                    </div>
                  </button>
                </div>
                {userMenuOpen && (
                  <div
                    ref={userMenuRef}
                    className="absolute right-0 w-48 py-1 mt-2 origin-top-right bg-white rounded-md shadow-lg ring-1 ring-black ring-opacity-5 focus:outline-none"
                  >
<button
    onClick={openChangePwd}
    className="block w-full px-4 py-2 text-sm text-left text-gray-700 hover:bg-gray-100"
>
    修改密码
</button>
                    <button
                      onClick={handleLogout}
                      className="block w-full px-4 py-2 text-sm text-left text-gray-700 hover:bg-gray-100"
                    >
                      登出
                    </button>
                  </div>
                )}
              </div>
            </div>
          ) : (
            <div className="hidden sm:ml-6 sm:flex sm:items-center">
              <Link
                to="/login"
                className="inline-flex items-center px-4 py-2 text-sm font-medium text-white bg-indigo-600 border border-transparent rounded-md shadow-sm hover:bg-indigo-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500"
              >
                登录
              </Link>
            </div>
          )}
        </div>
      </div>
    {showChangePwd && (
                <div className="fixed inset-0 z-50 flex items-center justify-center bg-gray-500 bg-opacity-75">
                    <div className="w-full max-w-md p-6 bg-white rounded-lg shadow-xl">
                        <h3 className="mb-4 text-lg font-medium text-gray-900">修改密码</h3>
                        {pwdError && (
                            <div className="p-3 mb-4 text-sm text-red-700 bg-red-50 border border-red-200 rounded-md">
                                {pwdError}
                            </div>
                        )}
                        <form onSubmit={handleChangePassword}>
                            <div className="mb-4">
                                <label className="block mb-1 text-sm font-medium text-gray-700">旧密码</label>
                                <input
                                    type="password"
                                    value={oldPassword}
                                    onChange={(e) => setOldPassword(e.target.value)}
                                    required
                                    autoComplete="current-password"
                                    className="block w-full px-3 py-2 border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-indigo-500 focus:border-indigo-500 sm:text-sm"
                                />
                            </div>
                            <div className="mb-4">
                                <label className="block mb-1 text-sm font-medium text-gray-700">新密码（至少 4 位）</label>
                                <input
                                    type="password"
                                    value={newPassword}
                                    onChange={(e) => setNewPassword(e.target.value)}
                                    required
                                    autoComplete="new-password"
                                    className="block w-full px-3 py-2 border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-indigo-500 focus:border-indigo-500 sm:text-sm"
                                />
                            </div>
                            <div className="mb-6">
                                <label className="block mb-1 text-sm font-medium text-gray-700">确认新密码</label>
                                <input
                                    type="password"
                                    value={confirmPassword}
                                    onChange={(e) => setConfirmPassword(e.target.value)}
                                    required
                                    autoComplete="new-password"
                                    className="block w-full px-3 py-2 border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-indigo-500 focus:border-indigo-500 sm:text-sm"
                                />
                            </div>
                            <div className="flex justify-end space-x-3">
                                <button
                                    type="button"
                                    onClick={() => setShowChangePwd(false)}
                                    className="px-4 py-2 text-sm font-medium text-gray-700 bg-white border border-gray-300 rounded-md shadow-sm hover:bg-gray-50"
                                >
                                    取消
                                </button>
                                <button
                                    type="submit"
                                    disabled={pwdSubmitting}
                                    className="px-4 py-2 text-sm font-medium text-white bg-indigo-600 border border-transparent rounded-md shadow-sm hover:bg-indigo-700 disabled:opacity-50"
                                >
                                    {pwdSubmitting ? '提交中...' : '确认修改'}
                                </button>
                            </div>
                        </form>
                    </div>
                </div>
            )}
            </nav>
  )
}

export default Navigation
