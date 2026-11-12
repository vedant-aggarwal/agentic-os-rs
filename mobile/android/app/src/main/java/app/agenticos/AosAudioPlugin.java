package app.agenticos;

import android.content.Context;
import android.media.AudioDeviceInfo;
import android.media.AudioManager;
import android.os.Build;

import com.getcapacitor.JSObject;
import com.getcapacitor.Plugin;
import com.getcapacitor.PluginCall;
import com.getcapacitor.PluginMethod;
import com.getcapacitor.annotation.CapacitorPlugin;

import java.util.List;

/**
 * The call screen's audio route (app/js/svc/native-plugins.js, native.audio). Her voice plays through the WebView's
 * WebRTC track; this only moves it between the loudspeaker and the earpiece.
 *   begin({ speaker })  the call starts: communication mode (so the earpiece exists), then the chosen output
 *   setSpeaker({ on })  the Speaker button
 *   end()               the call is over: the phone's audio mode and route go back to how they were
 * Each answers { speaker, route: 'speaker' | 'earpiece' | 'bluetooth' | 'wired' | 'other' }.
 * Android 12+ uses setCommunicationDevice; older phones use setSpeakerphoneOn.
 */
@CapacitorPlugin(name = "AosAudio")
public class AosAudioPlugin extends Plugin {
    private Integer savedMode = null;
    private boolean savedSpeaker = false;

    private AudioManager am() {
        return (AudioManager) getContext().getSystemService(Context.AUDIO_SERVICE);
    }

    @PluginMethod
    public void begin(PluginCall call) {
        AudioManager am = am();
        if (am == null) { call.reject("No audio service", "UNAVAILABLE"); return; }
        if (savedMode == null) {
            savedMode = am.getMode();
            savedSpeaker = am.isSpeakerphoneOn();
        }
        try { am.setMode(AudioManager.MODE_IN_COMMUNICATION); } catch (Exception ignored) {}
        route(am, call.getBoolean("speaker", true));
        call.resolve(state(am));
    }

    @PluginMethod
    public void setSpeaker(PluginCall call) {
        AudioManager am = am();
        if (am == null) { call.reject("No audio service", "UNAVAILABLE"); return; }
        if (am.getMode() != AudioManager.MODE_IN_COMMUNICATION) {
            if (savedMode == null) { savedMode = am.getMode(); savedSpeaker = am.isSpeakerphoneOn(); }
            try { am.setMode(AudioManager.MODE_IN_COMMUNICATION); } catch (Exception ignored) {}
        }
        route(am, call.getBoolean("on", true));
        call.resolve(state(am));
    }

    @PluginMethod
    public void end(PluginCall call) {
        AudioManager am = am();
        if (am == null) { call.resolve(); return; }
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) am.clearCommunicationDevice();
            else am.setSpeakerphoneOn(savedSpeaker);
            am.setMode(savedMode != null ? savedMode : AudioManager.MODE_NORMAL);
        } catch (Exception ignored) {}
        savedMode = null;
        call.resolve(state(am));
    }

    @Override
    protected void handleOnDestroy() {
        AudioManager am = am();
        if (am != null && savedMode != null) {
            try {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) am.clearCommunicationDevice();
                am.setMode(savedMode);
            } catch (Exception ignored) {}
        }
        super.handleOnDestroy();
    }

    private void route(AudioManager am, boolean speaker) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            int want = speaker ? AudioDeviceInfo.TYPE_BUILTIN_SPEAKER : AudioDeviceInfo.TYPE_BUILTIN_EARPIECE;
            List<AudioDeviceInfo> devices = am.getAvailableCommunicationDevices();
            AudioDeviceInfo pick = null;
            // a headset or Bluetooth device wins over the earpiece: "speaker off" means "not the loudspeaker"
            if (!speaker) {
                for (AudioDeviceInfo d : devices) {
                    int t = d.getType();
                    if (t == AudioDeviceInfo.TYPE_WIRED_HEADSET || t == AudioDeviceInfo.TYPE_WIRED_HEADPHONES
                            || t == AudioDeviceInfo.TYPE_USB_HEADSET || t == AudioDeviceInfo.TYPE_BLUETOOTH_SCO
                            || t == AudioDeviceInfo.TYPE_BLE_HEADSET) { pick = d; break; }
                }
            }
            if (pick == null) for (AudioDeviceInfo d : devices) if (d.getType() == want) { pick = d; break; }
            if (pick != null) {
                try { am.setCommunicationDevice(pick); return; } catch (Exception ignored) {}
            }
        }
        try { am.setSpeakerphoneOn(speaker); } catch (Exception ignored) {}
    }

    private JSObject state(AudioManager am) {
        String route = "other";
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            AudioDeviceInfo d = am.getCommunicationDevice();
            int t = d != null ? d.getType() : -1;
            if (t == AudioDeviceInfo.TYPE_BUILTIN_SPEAKER) route = "speaker";
            else if (t == AudioDeviceInfo.TYPE_BUILTIN_EARPIECE) route = "earpiece";
            else if (t == AudioDeviceInfo.TYPE_BLUETOOTH_SCO || t == AudioDeviceInfo.TYPE_BLE_HEADSET) route = "bluetooth";
            else if (t == AudioDeviceInfo.TYPE_WIRED_HEADSET || t == AudioDeviceInfo.TYPE_WIRED_HEADPHONES || t == AudioDeviceInfo.TYPE_USB_HEADSET) route = "wired";
        } else {
            route = am.isSpeakerphoneOn() ? "speaker" : "earpiece";
        }
        JSObject r = new JSObject();
        r.put("speaker", "speaker".equals(route));
        r.put("route", route);
        return r;
    }
}
