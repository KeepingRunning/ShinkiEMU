package io.github.keepingrunning.leaftrial;

import org.libsdl.app.SDLActivity;

public final class PlayerActivity extends SDLActivity {
    @Override protected String[] getArguments() {
        return new String[] { getIntent().getStringExtra("directory"), getIntent().getStringExtra("mode") };
    }
}
