import { ModalDialog } from './dialog';

export interface HyperlinkInsertValues {
  url: string;
  displayText: string;
}

export const MAX_HYPERLINK_URL_LENGTH = 2048;
export const MAX_HYPERLINK_TEXT_LENGTH = 1024;

export function normalizeHyperlinkUrl(value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed || trimmed.length > MAX_HYPERLINK_URL_LENGTH) return null;
  if (/[\u0000-\u001f\u007f]/u.test(trimmed)) return null;
  return trimmed;
}

export class HyperlinkDialog extends ModalDialog {
  private urlInput!: HTMLInputElement;
  private textInput!: HTMLInputElement;
  private errorLabel!: HTMLDivElement;

  onApply: ((values: HyperlinkInsertValues) => void) | null = null;

  constructor() {
    super('하이퍼링크', 420);
  }

  protected createBody(): HTMLElement {
    const body = document.createElement('div');
    body.className = 'field-edit-body';

    const panel = document.createElement('div');
    panel.className = 'field-edit-panel';

    const urlLabel = document.createElement('label');
    urlLabel.className = 'field-edit-label';
    urlLabel.textContent = '연결 주소(U):';
    panel.appendChild(urlLabel);

    this.urlInput = document.createElement('input');
    this.urlInput.type = 'text';
    this.urlInput.inputMode = 'url';
    this.urlInput.setAttribute('autocomplete', 'url');
    this.urlInput.className = 'field-edit-input';
    this.urlInput.maxLength = MAX_HYPERLINK_URL_LENGTH;
    this.urlInput.placeholder = 'https://example.com';
    panel.appendChild(this.urlInput);

    const textLabel = document.createElement('label');
    textLabel.className = 'field-edit-label';
    textLabel.textContent = '표시할 문자열(T):';
    panel.appendChild(textLabel);

    this.textInput = document.createElement('input');
    this.textInput.type = 'text';
    this.textInput.className = 'field-edit-input';
    this.textInput.maxLength = MAX_HYPERLINK_TEXT_LENGTH;
    panel.appendChild(this.textInput);

    this.errorLabel = document.createElement('div');
    this.errorLabel.className = 'field-edit-error';
    this.errorLabel.style.color = '#c00';
    this.errorLabel.style.fontSize = '11px';
    this.errorLabel.style.display = 'none';
    panel.appendChild(this.errorLabel);

    this.urlInput.addEventListener('input', () => {
      if (!this.textInput.value) this.textInput.placeholder = this.urlInput.value.trim();
    });

    body.appendChild(panel);
    return body;
  }

  protected onConfirm(): void | boolean {
    const url = normalizeHyperlinkUrl(this.urlInput.value);
    if (!url) {
      this.errorLabel.textContent = `연결 주소를 입력해 주세요. (최대 ${MAX_HYPERLINK_URL_LENGTH}자)`;
      this.errorLabel.style.display = '';
      this.urlInput.focus();
      return false;
    }

    const displayText = this.textInput.value || url;
    if (!displayText.trim() || displayText.length > MAX_HYPERLINK_TEXT_LENGTH) {
      this.errorLabel.textContent = `표시할 문자열을 입력해 주세요. (최대 ${MAX_HYPERLINK_TEXT_LENGTH}자)`;
      this.errorLabel.style.display = '';
      this.textInput.focus();
      return false;
    }

    this.errorLabel.style.display = 'none';
    this.onApply?.({ url, displayText });
  }

  override show(): void {
    super.show();
    this.urlInput.focus();
  }
}
