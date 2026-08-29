/**
 * İşletim sistemi bildirimleri.
 *
 * Uygulama içi toast yalnızca pencere görünürken işe yarıyor; Muiget ise
 * kapatılınca tepsiye iniyor ve indirmeler arkada sürüyor. Bir indirme
 * bittiğinde kullanıcının haberi olmasının tek yolu bu.
 */

import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from '@tauri-apps/plugin-notification';

/**
 * İzin durumu bir kez sorulup saklanıyor.
 *
 * `null` = henüz sorulmadı. İzin açılışta değil ilk ihtiyaç anında isteniyor:
 * bir indirme yöneticisinin daha ilk saniyede bildirim izni istemesi, henüz
 * gösterecek bir şeyi yokken kullanıcıyı rahatsız etmek olurdu.
 */
let izinli: boolean | null = null;

/**
 * Bildirim gönderir. Gönderilebildiyse `true` döner.
 *
 * Hiçbir durumda hata fırlatmıyor: bildirim bir kolaylık, indirmenin kendisi
 * değil. İzin reddedildiyse ya da platform desteklemiyorsa çağıran taraf
 * `false` görüp uygulama içi bildirime düşüyor.
 */
export async function osBildirimi(baslik: string, govde: string): Promise<boolean> {
  try {
    if (izinli === null) {
      izinli = await isPermissionGranted();
      if (!izinli) izinli = (await requestPermission()) === 'granted';
    }
    if (!izinli) return false;

    sendNotification({ title: baslik, body: govde });
    return true;
  } catch {
    return false;
  }
}
