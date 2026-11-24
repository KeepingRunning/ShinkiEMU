import { common } from '@kit.AbilityKit';
import { BusinessError } from '@kit.BasicServicesKit';
import { fileUri, picker } from '@kit.CoreFileKit';
import leaf from 'libshinki_emu_manager.so';

@Entry
@Component
struct Index {
  @State private directory: string = '';
  @State private failure: string = '';
  private mode: string = 'main';
  private context = getContext(this) as common.UIAbilityContext;

  aboutToAppear(): void {
    leaf.setEventListener((kind: string, message: string) => {
      if (kind === 'closed' || kind === 'error') {
        this.directory = '';
        this.failure = message;
      }
    });
  }

  private async selectGame(mode: string): Promise<void> {
    try {
      const options = new picker.DocumentSelectOptions();
      options.maxSelectNumber = 1;
      options.selectMode = picker.DocumentSelectMode.FOLDER;
      const uris = await new picker.DocumentViewPicker(this.context).select(options);
      if (uris.length === 0) return;
      this.mode = mode;
      this.failure = '';
      this.directory = new fileUri.FileUri(uris[0]).path;
    } catch (error) {
      this.failure = (error as BusinessError).message;
    }
  }

  onBackPress(): boolean {
    if (!this.directory.length) return false;
    leaf.pressKey(2, true);
    return true;
  }

  build() {
    Stack() {
      if (this.directory.length > 0) {
        XComponent({ id: 'leaf', type: XComponentType.SURFACE, libraryname: 'shinki_emu_manager' })
          .width('100%').height('100%')
          .onLoad(() => { leaf.start(this.mode + '\n' + this.directory); })
      } else {
        Column({ space: 18 }) {
          Button('WHITE ALBUM2').onClick(() => { this.selectGame('main'); })
          Button('Special Contents').onClick(() => { this.selectGame('special'); })
          Text(this.failure.length > 0 ? this.failure : '请选择已解压的游戏目录').fontSize(16)
        }.width('100%').height('100%').justifyContent(FlexAlign.Center)
      }
    }.width('100%').height('100%')
  }
}
