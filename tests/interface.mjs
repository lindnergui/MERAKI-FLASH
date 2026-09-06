import { mkdirSync } from "node:fs";
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
mkdirSync(".qa", { recursive: true });
import assert from 'node:assert/strict';

const browser = await chromium.launch({channel:process.platform === 'win32' ? 'msedge' : undefined, headless:true});
const page = await browser.newPage({viewport:{width:1180,height:780}});
const errors=[];
page.on('pageerror', e=>errors.push(e.message));
await page.route('https://api.github.com/**', route=>route.fulfill({json:{tag_name:'v1.1.0',draft:false,prerelease:false}}));
await page.addInitScript(()=>{
  const callbacks=new Map(), listeners=new Map(); let next=1;
  window.__mock={count:0,confirm:true,fail:false,listeners};
  window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener(event,id){listeners.delete(id)}};
  window.__TAURI_INTERNALS__={
    metadata:{currentWindow:{label:'main'},currentWebview:{label:'main'}},
    transformCallback(fn){const id=next++;callbacks.set(id,fn);return id;},
    async invoke(cmd,args){
      if(cmd==='plugin:event|listen'){const id=next++;listeners.set(id,{event:args.event,fn:callbacks.get(args.handler)});return id;}
      if(cmd==='plugin:event|unlisten')return;
      if(cmd==='plugin:app|version')return '1.0.0';
      if(cmd==='list_usb_devices')return [{id:'usb1',name:'USB de teste',devicePath:'test-only',mountPoint:'E:\\',mountPoints:['E:\\'],fileSystem:'FAT32',totalBytes:16000000000,availableBytes:8000000000,readOnly:false,kind:'Removable',transport:'USB',serial:'test'}];
      if(cmd==='plugin:dialog|open')return 'C:\\ISOs\\'+('nome-muito-longo-'.repeat(12))+'.iso';
      if(cmd==='plugin:dialog|message')return window.__mock.confirm ? 'Apagar e gravar' : 'Cancelar';
      if(cmd==='start_flash'){window.__mock.count++; if(window.__mock.fail)throw new Error('UAC cancelado');return {operationId:'op1'};}
      if(cmd==='open_releases_page'){window.__mock.opened=true;return;}
      throw new Error('IPC inesperado: '+cmd);
    }
  };
  window.__emit=(phase,message,operationId='op1')=>{
    for(const {event,fn} of listeners.values())if(event==='flash-progress')fn({payload:{operationId,phase,message,percentage:phase==='done'?100:35,bytesPerSecond:1000000,etaSeconds:phase==='done'?0:60}});
  };
});
try{
 await page.goto('http://127.0.0.1:1420');
 await page.getByRole('heading',{name:'Seu sistema, pronto para uso',exact:true}).waitFor();
 assert.equal(await page.getByText('Alpha',{exact:true}).count(),0);
 assert.equal(await page.getByText('Modo de proteção ativo',{exact:true}).count(),0);
 await page.getByText('Meraki Flash v1.1.0 disponível.',{exact:false}).waitFor();
 await page.getByRole('button',{name:'Ver atualização',exact:true}).click();
 assert.equal(await page.evaluate(()=>window.__mock.opened),true);
 await page.getByRole('button',{name:'Fechar aviso de atualização',exact:true}).click();
 const step=async n=>assert.equal(await page.locator('[aria-current="step"]').textContent(),String(n));
 await step(1);
 await page.getByRole('button',{name:/Linux Ubuntu/}).click();await step(1);
 await page.getByRole('button',{name:/Clique para escolher uma ISO/}).click();await step(2);
 await page.getByRole('button',{name:/USB de teste/}).click();await step(3);
 await page.getByRole('button',{name:/Linux Ubuntu/}).click();await step(1);
 await page.getByRole('button',{name:/USB de teste/}).click();await step(3);
 for(const width of [880,1080,1180,1512]){
  await page.setViewportSize({width,height:780});
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,`overflow ${width}`);
  if(width>=1080){const widths=await page.locator('section.meraki-card').evaluateAll(nodes=>nodes.slice(0,3).map(n=>n.getBoundingClientRect().width));assert.ok(Math.max(...widths)/Math.min(...widths)<1.3,JSON.stringify(widths));}
 }
 await page.setViewportSize({width:1180,height:780});
 await page.getByRole('button',{name:'Gravar pendrive',exact:true}).click();
 await page.getByText('Aguardando autorização administrativa…',{exact:true}).waitFor();await step(4);
 assert.equal(await page.evaluate(()=>window.__mock.count),1);
 await page.evaluate(()=>window.__emit('preparing','Validando novamente o dispositivo…'));
 await page.getByText('Validando novamente o dispositivo…',{exact:true}).waitFor();
 assert.equal(await page.getByText('Aguardando autorização administrativa…',{exact:true}).count(),0);
 await page.evaluate(()=>window.__emit('writing','Gravando a imagem bit a bit…'));
 assert.equal(await page.locator('[aria-label="Etapa 4 selecionada"]').count(),1);
 assert.equal(await page.getByRole('button',{name:/Linux Ubuntu/}).isDisabled(),true);
 await page.evaluate(()=>window.__emit('error','evento antigo','other-operation'));
 assert.equal(await page.getByText('evento antigo',{exact:true}).count(),0);
 await page.screenshot({path:'.qa/interface-gravando.png',fullPage:true});
 await page.evaluate(()=>window.__emit('done','Pendrive gravado com sucesso.'));
 await page.getByText('Concluído',{exact:true}).waitFor();
 await page.getByRole('button',{name:/USB de teste/}).click();await step(3);
 await page.evaluate(()=>{window.__mock.fail=true;});
 await page.getByRole('button',{name:'Gravar pendrive',exact:true}).click();
 await page.getByText('A gravação foi interrompida',{exact:true}).waitFor();
 assert.equal(await page.getByRole('button',{name:'Gravar pendrive',exact:true}).isEnabled(),true);
 await page.getByRole('checkbox',{name:'Avisar sobre novas versões ao abrir'}).uncheck();
 await page.reload();
 assert.equal(await page.getByRole('checkbox',{name:'Avisar sobre novas versões ao abrir'}).isChecked(),false);
 assert.equal(await page.getByRole('button',{name:'Ver atualização',exact:true}).count(),0);
 assert.deepEqual(errors,[]);
 console.log('PASS: etapas, layouts 880/1080/1180/1512, autorização, bloqueios, conclusão, erro, atualização opcional e persistência.');
}finally{await browser.close();}


