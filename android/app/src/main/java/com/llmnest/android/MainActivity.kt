package com.llmnest.android

import android.app.Activity
import android.content.res.Configuration
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.view.View
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.widget.ImageView
import android.widget.RadioGroup
import android.widget.TextView

class MainActivity : Activity() {
    private val preferences by lazy { getSharedPreferences("navigation", MODE_PRIVATE) }
    private var contactsSelected = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)
        // PhoneWindow needs its decor view before accessing the insets controller.
        configureSystemBars()
        val root = findViewById<View>(R.id.root)
        root.setOnApplyWindowInsetsListener { view, insets ->
            if (Build.VERSION.SDK_INT >= 30) {
                val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
                view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
            } else {
                @Suppress("DEPRECATION")
                view.setPadding(insets.systemWindowInsetLeft, insets.systemWindowInsetTop,
                    insets.systemWindowInsetRight, insets.systemWindowInsetBottom)
            }
            insets
        }
        root.requestApplyInsets()
        contactsSelected = savedInstanceState?.getBoolean(STATE_CONTACTS)
            ?: preferences.getBoolean(STATE_CONTACTS, false)
        val navigation = findViewById<RadioGroup>(R.id.navigation)
        navigation.check(if (contactsSelected) R.id.tab_contacts else R.id.tab_messages)
        showPage()
        navigation.setOnCheckedChangeListener { _, checkedId ->
            contactsSelected = checkedId == R.id.tab_contacts
            preferences.edit().putBoolean(STATE_CONTACTS, contactsSelected).apply()
            showPage()
        }
    }

    private fun showPage() {
        findViewById<TextView>(R.id.page_title).setText(if (contactsSelected) R.string.contacts else R.string.messages)
        findViewById<TextView>(R.id.empty_title).setText(if (contactsSelected) R.string.contacts_empty else R.string.messages_empty)
        findViewById<TextView>(R.id.empty_hint).setText(if (contactsSelected) R.string.contacts_hint else R.string.messages_hint)
        findViewById<ImageView>(R.id.empty_icon).setImageResource(if (contactsSelected) R.drawable.ic_contacts else R.drawable.ic_message)
    }

    @Suppress("DEPRECATION")
    private fun configureSystemBars() {
        val isLight = resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK != Configuration.UI_MODE_NIGHT_YES
        window.statusBarColor = Color.TRANSPARENT
        window.navigationBarColor = Color.TRANSPARENT
        if (Build.VERSION.SDK_INT >= 30) {
            window.setDecorFitsSystemWindows(false)
            val appearance = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS
            window.insetsController?.setSystemBarsAppearance(if (isLight) appearance else 0, appearance)
        } else {
            window.decorView.systemUiVisibility = View.SYSTEM_UI_FLAG_LAYOUT_STABLE or
                View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN or View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION or
                (if (isLight) View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR or View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR else 0)
        }
    }

    override fun onSaveInstanceState(outState: Bundle) {
        outState.putBoolean(STATE_CONTACTS, contactsSelected)
        super.onSaveInstanceState(outState)
    }

    companion object {
        private const val STATE_CONTACTS = "contacts_selected"
    }
}
