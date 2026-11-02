// Serves the Rust Manager's requests (Platforms/OHOS/native/bridge.c ->
// shinki_ohos_notify) and feeds ability state back into it.
import { common, ConfigurationConstant, Want } from '@kit.AbilityKit';
import { BusinessError } from '@kit.BasicServicesKit';
import { fileIo, fileUri, fileShare, picker } from '@kit.CoreFileKit';
import { display, promptAction } from '@kit.ArkUI';
import { pointer } from '@kit.InputKit';
import { util } from '@kit.ArkTS';
import { hilog } from '@kit.PerformanceAnalysisKit';
import manager from 'libshinki_emu_manager.so';

const DOMAIN = 0xD001;
const TAG = 'ShinkiEMU';
const BUNDLED = 'bundled';

interface PickRequest {
  id: number;
  title: string;
  extensions: string[];
  defaultName: string;
}

/** Reports the color mode and the default display's vp-to-px ratio. */
export function reportConfiguration(colorMode: ConfigurationConstant.ColorMode | undefined): void {
  const dark = colorMode === ConfigurationConstant.ColorMode.COLOR_MODE_DARK;
  manager.configure(dark, display.getDefaultDisplaySync().densityPixels);
}

/**
 * Replaces filesDir/bundled with the rawfile resources listed in
 * rawfile/bundled/bundled.manifest (metadata snapshot, licenses, notices).
 */
export function extractBundled(context: common.UIAbilityContext): void {
  const target = `${context.filesDir}/${BUNDLED}`;
  if (fileIo.accessSync(target)) {
    fileIo.rmdirSync(target);
  }
  const resources = context.resourceManager;
  const manifest: string = util.TextDecoder.create('utf-8')
    .decodeToString(resources.getRawFileContentSync(`${BUNDLED}/bundled.manifest`));
  for (const entry of manifest.split('\n')) {
    if (entry.length === 0) {
      continue;
    }
    const destination = `${target}/${entry}`;
    fileIo.mkdirSync(destination.substring(0, destination.lastIndexOf('/')), true);
    const bytes = resources.getRawFileContentSync(`${BUNDLED}/${entry}`);
    const file = fileIo.openSync(destination, fileIo.OpenMode.CREATE | fileIo.OpenMode.WRITE_ONLY | fileIo.OpenMode.TRUNC);
    try {
      fileIo.writeSync(file.fd, bytes.buffer);
    } finally {
      fileIo.closeSync(file);
    }
  }
}

/** Installs the Manager event listener; call before `manager.start`. */
export function listen(context: common.UIAbilityContext): void {
  manager.setEventListener((kind: string, payload: string) => {
    switch (kind) {
      case 'error':
        hilog.error(DOMAIN, TAG, '%{public}s', payload);
        promptAction.showToast({ message: payload, duration: 5000 });
        break;
      case 'exit':
        context.terminateSelf();
        break;
      case 'restart':
        restart(context);
        break;
      case 'openPath':
        openPath(context, payload);
        break;
      case 'openUrl':
        context.startAbility({ action: 'ohos.want.action.viewData', uri: payload })
          .catch((error: BusinessError) => {
            promptAction.showToast({ message: error.message, duration: 5000 });
          });
        break;
      case 'pointerVisible':
        pointer.setPointerVisible(payload === '1').catch((error: BusinessError) => {
          hilog.warn(DOMAIN, TAG, 'setPointerVisible failed: %{public}s', error.message);
        });
        break;
      case 'pickFolder':
      case 'pickFile':
      case 'pickNewDirectory':
        pick(context, kind, JSON.parse(payload) as PickRequest);
        break;
      default:
        hilog.warn(DOMAIN, TAG, 'unknown Manager request %{public}s', kind);
    }
  });
}

function restart(context: common.UIAbilityContext): void {
  const want: Want = { bundleName: context.abilityInfo.bundleName, abilityName: context.abilityInfo.name };
  try {
    context.getApplicationContext().restartApp(want);
  } catch (error) {
    promptAction.showToast({ message: (error as BusinessError).message, duration: 5000 });
  }
}

function openPath(context: common.UIAbilityContext, path: string): void {
  const want: Want = { action: 'ohos.want.action.viewData', uri: fileUri.getUriFromPath(path) };
  context.startAbility(want).catch((error: BusinessError) => {
    promptAction.showToast({ message: `${path}: ${error.message}`, duration: 5000 });
  });
}

/** Keeps access to a picked location across restarts where the device allows it. */
function persist(uri: string): void {
  const policy: fileShare.PolicyInfo = {
    uri: uri,
    operationMode: fileShare.OperationMode.READ_MODE | fileShare.OperationMode.WRITE_MODE
  };
  fileShare.persistPermission([policy]).catch((error: BusinessError) => {
    hilog.info(DOMAIN, TAG, 'access to %{public}s lasts for this session: %{public}s', uri, error.message);
  });
}

async function pick(context: common.UIAbilityContext, kind: string, request: PickRequest): Promise<void> {
  try {
    const options = new picker.DocumentSelectOptions();
    options.maxSelectNumber = 1;
    if (kind === 'pickFile') {
      if (request.extensions.length > 0) {
        options.fileSuffixFilters = [`${request.title}|${request.extensions.map((extension: string) => `.${extension}`).join(',')}`];
      }
    } else {
      options.selectMode = picker.DocumentSelectMode.FOLDER;
    }
    const uris = await new picker.DocumentViewPicker(context).select(options);
    if (uris.length === 0) {
      manager.completePicker(request.id, '', 'cancelled');
      return;
    }
    persist(uris[0]);
    const path = new fileUri.FileUri(uris[0]).path;
    // A new directory is created by the Manager inside the chosen parent.
    manager.completePicker(request.id, kind === 'pickNewDirectory' ? `${path}/${request.defaultName}` : path, '');
  } catch (error) {
    manager.completePicker(request.id, '', `SHINKI_EMU_OHOS_PICKER: ${(error as BusinessError).message}`);
  }
}
