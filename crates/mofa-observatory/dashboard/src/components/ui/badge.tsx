import React from 'react'
import { cva, type VariantProps } from 'class-variance-authority'
import { clsx } from 'clsx'

const badgeVariants = cva(
  'inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-semibold transition-colors',
  {
    variants: {
      variant: {
        default: 'bg-gray-700 text-gray-200',
        ok: 'bg-emerald-900/60 text-emerald-300 border border-emerald-700/50',
        error: 'bg-red-900/60 text-red-300 border border-red-700/50',
        unset: 'bg-gray-700/60 text-gray-400 border border-gray-600/50',
        warning: 'bg-yellow-900/60 text-yellow-300 border border-yellow-700/50',
        info: 'bg-blue-900/60 text-blue-300 border border-blue-700/50',
        anomaly: 'bg-red-600 text-white border border-red-500 font-bold',
        user: 'bg-blue-900/60 text-blue-300 border border-blue-700/50',
        assistant: 'bg-emerald-900/60 text-emerald-300 border border-emerald-700/50',
        tool: 'bg-orange-900/60 text-orange-300 border border-orange-700/50',
      },
    },
    defaultVariants: {
      variant: 'default',
    },
  },
)

interface BadgeProps
  extends React.HTMLAttributes<HTMLSpanElement>,
    VariantProps<typeof badgeVariants> {}

export function Badge({ className, variant, ...props }: BadgeProps) {
  return (
    <span className={clsx(badgeVariants({ variant }), className)} {...props} />
  )
}
