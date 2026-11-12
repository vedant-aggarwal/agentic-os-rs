package app.agenticos;

import android.os.Bundle;

import com.getcapacitor.BridgeActivity;

public class MainActivity extends BridgeActivity {
    @Override
    public void onCreate(Bundle savedInstanceState) {
        // the app's own plugins register before the bridge starts
        registerPlugin(AosAudioPlugin.class);
        super.onCreate(savedInstanceState);
    }
}
