const root = document.querySelector<HTMLElement>('#app')
if (!root) {
  throw new Error('Missing application root')
}

const label = document.createElement('label')
label.className = 'visually-hidden'
label.htmlFor = 'launcher-query'
label.textContent = '搜索应用'

const input = document.createElement('input')
input.id = 'launcher-query'
input.type = 'search'
input.placeholder = '搜索应用'
input.autocomplete = 'off'
input.spellcheck = false

root.append(label, input)
