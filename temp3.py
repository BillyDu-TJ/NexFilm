import sys
import re

with open('src/app_state.rs', 'r', encoding='utf-8') as f:
    app_state = f.read()

app_state = app_state.replace('#[derive(Debug, Clone, Serialize)]\npub struct BaseColor', '#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct BaseColor')

with open('src/app_state.rs', 'w', encoding='utf-8') as f:
    f.write(app_state)

with open('src/commands.rs', 'r', encoding='utf-8') as f:
    content = f.read()

content = content.replace('original_proxy: proxy.clone(),\n                    proxy_image: proxy,\n                    pristine_proxy,', 'original_proxy: Some(proxy.clone()),\n                    proxy_image: Some(proxy),\n                    pristine_proxy: Some(pristine_proxy),')
content = content.replace('item.pristine_proxy = compute_pristine_proxy(&proxy, &item.base_color, item.params.film_mode.clone());', 'item.pristine_proxy = Some(compute_pristine_proxy(&proxy, &item.base_color, item.params.film_mode.clone()));')
content = content.replace('let mut current = item.original_proxy.clone().unwrap().unwrap();', 'let mut current = item.original_proxy.clone().unwrap();')
content = content.replace('item.pristine_proxy = compute_pristine_proxy(item.proxy_image.as_ref().unwrap(), &item.base_color, item.params.film_mode.clone());', 'item.pristine_proxy = Some(compute_pristine_proxy(item.proxy_image.as_ref().unwrap(), &item.base_color, item.params.film_mode.clone()));')

with open('src/commands.rs', 'w', encoding='utf-8') as f:
    f.write(content)
