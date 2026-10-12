import { useTranslation } from 'react-i18next'
import { Toast, ToastAction, ToastClose, ToastDescription, ToastTitle, ToastViewport, useToastManager } from '@/components/ui/toast'

/** Where the "recorded" and "closed" notes of the application stack up. */
export function Toasts() {
  const { t } = useTranslation()
  const { toasts } = useToastManager()
  return (
    // Named in the interface's language: Base UI calls the region
    // "Notifications" otherwise, in English, whatever the screen is in. On a
    // phone they stack above the bottom bar rather than over it - the bar is
    // where the thumb goes next.
    <ToastViewport aria-label={t('common.notifications')} className="max-sm:bottom-[calc(5rem+env(safe-area-inset-bottom))]">
      {toasts.map((toast) => (
        <Toast key={toast.id} toast={toast}>
          <ToastTitle />
          <ToastDescription />
          {/* Drawn only for a toast raised with something to do - Undo. */}
          <ToastAction />
          <ToastClose aria-label={t('common.dismiss')} />
        </Toast>
      ))}
    </ToastViewport>
  )
}
