/**
 * Format the status-bar page label without confusing logical HWP page numbers
 * with the physical pagination index.
 */
export function formatStatusPageLabel(
  pageIndex: number,
  totalPhysicalPages: number,
  logicalPageNumber?: number,
): string {
  const physical = pageIndex + 1;
  const logical = logicalPageNumber && logicalPageNumber > 0 ? logicalPageNumber : physical;
  if (logical === physical) return `${physical} / ${totalPhysicalPages} 쪽`;
  return `${logical} 쪽 · ${physical} / ${totalPhysicalPages}`;
}
