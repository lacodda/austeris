import { useTranslation } from 'react-i18next'
import { Toast, ToastClose, ToastDescription, ToastTitle, ToastViewport, useToastManager } from '@/components/ui/toast'

/** Where the "recorded" and "closed" notes of the application stack up. */
export function Toasts() {
  const { t } = useTranslation()
  const { toasts } = useToastManager()
  return (
    <ToastViewport>
      {toasts.map((toast) => (
        <Toast key={toast.id} toast={toast}>
          <ToastTitle />
          <ToastDescription />
          <ToastClose aria-label={t('common.dismiss')} />
        </Toast>
      ))}
    </ToastViewport>
  )
}
