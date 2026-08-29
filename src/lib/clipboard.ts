/**
 * Panoya kopyalama.
 *
 * `navigator.clipboard` güvenli bağlam istiyor ve Tauri'nin özel şemasında bu
 * her platformda garanti değil. Başarısız olursa gizli bir `textarea` +
 * `execCommand` yoluna düşülüyor: eski bir API ama webview'lerde çalışıyor ve
 * "kopyalandı" derken kopyalamamış olmak en kötü sonuç olurdu.
 */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // Yedek yola düş.
  }

  try {
    const alan = document.createElement('textarea');
    alan.value = text;
    alan.setAttribute('readonly', '');
    alan.style.position = 'fixed';
    alan.style.top = '-1000px';
    alan.style.opacity = '0';
    document.body.appendChild(alan);
    alan.select();
    const oldu = document.execCommand('copy');
    document.body.removeChild(alan);
    return oldu;
  } catch {
    return false;
  }
}
