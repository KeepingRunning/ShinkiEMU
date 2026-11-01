package org.shinkirou.shinkiemu

import android.app.Dialog
import android.app.NativeActivity
import android.content.ActivityNotFoundException
import android.content.ComponentName
import android.content.Intent
import android.net.Uri
import android.view.PointerIcon
import android.webkit.CookieManager
import android.webkit.WebStorage
import android.webkit.WebView
import android.webkit.WebViewClient
import java.io.File
import org.shinkirou.krkr.KrkrHost

/**
 * NativeActivity host for the Rust Manager (`android_main` in libshinki_emu_manager).
 * Methods without `external` are called from the Slint event-loop thread over JNI;
 * `external` functions are implemented in `platform/android.rs`.
 */
class MainActivity : NativeActivity() {
    companion object {
        init {
            // The framework loads the lib for android.app.NativeActivity under
            // the boot classloader; loading it here also registers it for this
            // app classloader so the `external fun` bridges can bind.
            System.loadLibrary("shinki_emu_manager")
        }

        /** `pickDocument` kinds; must match `PICK_TREE` / `PICK_FILE` in Rust. */
        private const val PICK_TREE = 0
        private const val PICK_FILE = 1
        private const val FIRST_REQUEST_CODE = 0xA500
    }

    override fun onResume() {
        super.onResume()
        KrkrHost.onResume()
    }

    override fun onPause() {
        KrkrHost.onPause()
        super.onPause()
    }

    override fun onDestroy() {
        // NativeActivity's teardown completes the native app/owner before
        // the private core helpers release their Java Activity references.
        super.onDestroy()
        KrkrHost.destroy(this)
    }

    private class PendingPick(val requestId: Long, val kind: Int, val destDir: File)

    // UI thread only.
    private val pendingPicks = HashMap<Int, PendingPick>()
    private var nextRequestCode = FIRST_REQUEST_CODE

    /**
     * Completes `pickDocument`: `path` is the copied file or directory, `error`
     * a failure message; both null when the user cancelled.
     */
    private external fun nativeOnPickResult(requestId: Long, path: String?, error: String?)

    /**
     * Open the Storage Access Framework picker and copy the picked document
     * (kind 1) or tree (kind 0) below `destDir`, which must not exist yet.
     */
    fun pickDocument(requestId: Long, kind: Int, destDir: String) {
        runOnUiThread {
            val intent = when (kind) {
                PICK_TREE -> Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
                PICK_FILE -> Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                    addCategory(Intent.CATEGORY_OPENABLE)
                    type = "*/*"
                }
                else -> {
                    nativeOnPickResult(requestId, null, "unknown picker kind $kind")
                    return@runOnUiThread
                }
            }.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            val requestCode = nextRequestCode
            nextRequestCode = if (requestCode == 0xFFFF) FIRST_REQUEST_CODE else requestCode + 1
            pendingPicks[requestCode] = PendingPick(requestId, kind, File(destDir))
            try {
                startActivityForResult(intent, requestCode)
            } catch (error: ActivityNotFoundException) {
                pendingPicks.remove(requestCode)
                nativeOnPickResult(requestId, null, "no document picker: ${error.message}")
            }
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        val pick = pendingPicks.remove(requestCode) ?: return
        val uri: Uri? = data?.data
        if (resultCode != RESULT_OK || uri == null) {
            nativeOnPickResult(pick.requestId, null, null)
            return
        }
        // Copies can be large; keep the UI thread free. The result is posted
        // back to the Slint event loop by the native side.
        Thread({
            val result = runCatching {
                when (pick.kind) {
                    PICK_TREE -> ShinkiPlatformBridge.copyTree(this, uri, pick.destDir)
                    else -> ShinkiPlatformBridge.copyDocument(this, uri, pick.destDir)
                }
            }
            result.fold(
                onSuccess = { nativeOnPickResult(pick.requestId, it.absolutePath, null) },
                onFailure = {
                    pick.destDir.deleteRecursively()
                    nativeOnPickResult(pick.requestId, null, it.toString())
                },
            )
        }, "shinki-pick-copy").start()
    }

    /** Relaunch the activity in a fresh process. */
    fun restartApp() {
        runOnUiThread {
            startActivity(
                Intent.makeRestartActivityTask(ComponentName(this, MainActivity::class.java))
            )
            Runtime.getRuntime().exit(0)
        }
    }

    /** Hide or show the mouse pointer over the activity window. */
    fun setPointerHidden(hidden: Boolean) {
        runOnUiThread {
            val type = if (hidden) PointerIcon.TYPE_NULL else PointerIcon.TYPE_ARROW
            window.decorView.pointerIcon = PointerIcon.getSystemIcon(this, type)
        }
    }

    /** Page shown in the Store sign-in WebView; null while none is open. */
    @Volatile
    private var webLoginPage: String? = null

    // UI thread only.
    private var webLoginDialog: Dialog? = null
    private var webLoginView: WebView? = null

    /**
     * Show the Store sign-in page in a full-screen WebView. The cookie jar is
     * wiped first, so only this sign-in's cookies are read back; back closes it.
     */
    fun openWebLogin(url: String) {
        webLoginPage = url
        runOnUiThread {
            dismissWebLogin()
            val cookies = CookieManager.getInstance()
            cookies.removeAllCookies(null)
            cookies.setAcceptCookie(true)
            val view = WebView(this)
            cookies.setAcceptThirdPartyCookies(view, true)
            view.settings.javaScriptEnabled = true
            view.settings.domStorageEnabled = true
            view.webViewClient = object : WebViewClient() {
                override fun doUpdateVisitedHistory(view: WebView, url: String, isReload: Boolean) {
                    if (webLoginView === view) webLoginPage = url
                }

                override fun onPageFinished(view: WebView, url: String) {
                    if (webLoginView === view) webLoginPage = url
                }
            }
            val dialog = Dialog(this, android.R.style.Theme_DeviceDefault_NoActionBar)
            dialog.setContentView(view)
            dialog.setOnCancelListener {
                webLoginPage = null
                dismissWebLogin()
            }
            webLoginView = view
            webLoginDialog = dialog
            dialog.show()
            view.loadUrl(url)
        }
    }

    fun webLoginPage(): String? = webLoginPage

    /** `Cookie` header the sign-in WebView sends to [url], HttpOnly cookies included. */
    fun webLoginCookies(url: String): String? = CookieManager.getInstance().getCookie(url)

    /** Close the sign-in WebView and wipe its cookies and site data. */
    fun closeWebLogin() {
        webLoginPage = null
        runOnUiThread { dismissWebLogin() }
    }

    private fun dismissWebLogin() {
        webLoginDialog?.dismiss()
        webLoginDialog = null
        webLoginView?.let { view ->
            view.stopLoading()
            view.clearCache(true)
            view.destroy()
        }
        webLoginView = null
        CookieManager.getInstance().removeAllCookies(null)
        CookieManager.getInstance().flush()
        WebStorage.getInstance().deleteAllData()
    }
}
