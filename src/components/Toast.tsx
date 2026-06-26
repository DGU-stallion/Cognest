import { useCallback, useEffect, useRef, useState } from 'react';
import './Toast.css';

export interface ToastMessage {
  id: string;
  text: string;
  type: 'success' | 'error';
}

interface ToastItemProps {
  toast: ToastMessage;
  onRemove: (id: string) => void;
}

function ToastItem({ toast, onRemove }: ToastItemProps) {
  const [exiting, setExiting] = useState(false);

  useEffect(() => {
    const timer = setTimeout(() => setExiting(true), 1800);
    const removeTimer = setTimeout(() => onRemove(toast.id), 2000);
    return () => {
      clearTimeout(timer);
      clearTimeout(removeTimer);
    };
  }, [toast.id, onRemove]);

  return (
    <div className={`toast toast--${toast.type}${exiting ? ' toast--exiting' : ''}`}>
      {toast.text}
    </div>
  );
}

// Global toast state
let globalAddToast: ((text: string, type: 'success' | 'error') => void) | null = null;

export function showToast(text: string, type: 'success' | 'error' = 'success') {
  globalAddToast?.(text, type);
}

export default function ToastContainer() {
  const [toasts, setToasts] = useState<ToastMessage[]>([]);
  const idCounter = useRef(0);

  const addToast = useCallback((text: string, type: 'success' | 'error') => {
    const id = `toast-${++idCounter.current}`;
    setToasts((prev) => [...prev, { id, text, type }]);
  }, []);

  const removeToast = useCallback((id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  useEffect(() => {
    globalAddToast = addToast;
    return () => {
      globalAddToast = null;
    };
  }, [addToast]);

  if (toasts.length === 0) return null;

  return (
    <div className="toast-container" role="status" aria-live="polite">
      {toasts.map((t) => (
        <ToastItem key={t.id} toast={t} onRemove={removeToast} />
      ))}
    </div>
  );
}
