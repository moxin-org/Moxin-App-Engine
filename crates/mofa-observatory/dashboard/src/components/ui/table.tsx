import React from 'react'
import { clsx } from 'clsx'

interface TableProps extends React.HTMLAttributes<HTMLTableElement> {
  children: React.ReactNode
}

export function Table({ className, children, ...props }: TableProps) {
  return (
    <div className="relative w-full overflow-auto scrollbar-thin">
      <table
        className={clsx('w-full caption-bottom text-sm', className)}
        {...props}
      >
        {children}
      </table>
    </div>
  )
}

export function TableHeader({
  className,
  children,
  ...props
}: React.HTMLAttributes<HTMLTableSectionElement> & { children: React.ReactNode }) {
  return (
    <thead className={clsx('[&_tr]:border-b [&_tr]:border-gray-700', className)} {...props}>
      {children}
    </thead>
  )
}

export function TableBody({
  className,
  children,
  ...props
}: React.HTMLAttributes<HTMLTableSectionElement> & { children: React.ReactNode }) {
  return (
    <tbody
      className={clsx('[&_tr:last-child]:border-0', className)}
      {...props}
    >
      {children}
    </tbody>
  )
}

export function TableRow({
  className,
  children,
  ...props
}: React.HTMLAttributes<HTMLTableRowElement> & { children: React.ReactNode }) {
  return (
    <tr
      className={clsx(
        'border-b border-gray-700/50 transition-colors hover:bg-gray-700/30 data-[state=selected]:bg-gray-700',
        className,
      )}
      {...props}
    >
      {children}
    </tr>
  )
}

export function TableHead({
  className,
  children,
  ...props
}: React.ThHTMLAttributes<HTMLTableCellElement> & { children?: React.ReactNode }) {
  return (
    <th
      className={clsx(
        'h-12 px-4 text-left align-middle font-medium text-gray-400 [&:has([role=checkbox])]:pr-0',
        className,
      )}
      {...props}
    >
      {children}
    </th>
  )
}

export function TableCell({
  className,
  children,
  ...props
}: React.TdHTMLAttributes<HTMLTableCellElement> & { children?: React.ReactNode }) {
  return (
    <td
      className={clsx(
        'px-4 py-3 align-middle [&:has([role=checkbox])]:pr-0',
        className,
      )}
      {...props}
    >
      {children}
    </td>
  )
}
