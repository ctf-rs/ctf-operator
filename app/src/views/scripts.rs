/// Emits the inline JavaScript that polls `/challenge/{name}/instance-state`
/// every 5 seconds and counts down the timer locally every second.
/// Also adds instant button hide on click for spawn/stop/renew.
pub fn instance_live_script(challenge_name: &str) -> String {
    let endpoint = format!(
        "/challenge/{}/instance-state",
        urlencoding::encode(challenge_name),
    );

    format!(
        r#"(function(){{const endpoint='{endpoint}';const statusEl=document.getElementById('instance-status');let timerEl=document.getElementById('instance-timer');const extendEl=document.getElementById('extend-form');const controlsEl=document.getElementById('instance-controls');let lastState=null;let localSeconds=null;let wasExpired=false;function fmt(s){{const h=Math.floor(s/3600);const m=Math.floor((s%3600)/60);const sec=s%60;if(h>0)return`${{h}}h ${{m}}m ${{sec}}s`;if(m>0)return`${{m}}m ${{sec}}s`;return`${{sec}}s`;}}function setStatus(state){{if(!statusEl)return;statusEl.classList.remove('ready','deleting');if(state.solved){{statusEl.textContent='Solved';return;}}if(state.deleting){{statusEl.textContent='Stopping...';statusEl.classList.add('deleting');return;}}if(!state.has_instance){{statusEl.textContent='No instance';return;}}if(!state.ready){{if(typeof state.time_left_seconds==='number'&&state.time_left_seconds===0){{statusEl.textContent='Expired';}}else{{statusEl.textContent='Starting...';}}return;}}statusEl.textContent='Ready';statusEl.classList.add('ready');}}function updateTimer(){{timerEl=document.getElementById('instance-timer');if(!timerEl)return;if(localSeconds!==null&&localSeconds>0){{timerEl.textContent=fmt(localSeconds);timerEl.style.display='';}}else{{timerEl.style.display='none';}}}}function tick(){{if(localSeconds===null)return;localSeconds=Math.max(0,localSeconds-1);updateTimer();if(localSeconds===0&&!wasExpired){{wasExpired=true;window.location.reload();return;}}if(extendEl)extendEl.style.display=(localSeconds<=300&&localSeconds>0)?'block':'none';}}async function poll(){{try{{const res=await fetch(endpoint,{{credentials:'same-origin',cache:'no-store'}});if(!res.ok)return;const state=await res.json();setStatus(state);const expired=state.has_instance&&(typeof state.time_left_seconds==='number')&&state.time_left_seconds===0;const key=`${{state.solved}}|${{state.has_instance}}|${{state.ready}}|${{state.deleting}}|${{state.instance_name||''}}|${{expired}}`;if(lastState!==null&&key!==lastState){{window.location.reload();return;}}lastState=key;if(state.solved||!state.has_instance||!state.ready||state.deleting){{localSeconds=null;updateTimer();if(extendEl)extendEl.style.display='none';return;}}if(typeof state.time_left_seconds==='number'){{localSeconds=state.time_left_seconds;updateTimer();}}else{{localSeconds=null;updateTimer();}}if(extendEl)extendEl.style.display=state.can_extend?'block':'none';}}catch(_err){{}}}}function setupButtonHide(){{['spawn-form','stop-form','extend-form'].forEach(id=>{{const form=document.getElementById(id);if(form){{form.addEventListener('submit',function(){{if(controlsEl)controlsEl.style.opacity='0.5';const btn=form.querySelector('button');if(btn){{btn.disabled=true;btn.textContent='...';}}if(statusEl)statusEl.textContent='Processing...';}})}}}});}}setInterval(tick,1000);setInterval(poll,5000);poll();setupButtonHide();}})();"#,
    )
}

/// Emits JavaScript for the index page that counts down all card timers every second.
/// This parses the current timer text, decrements, and updates. Refreshes page every 30s.
pub fn index_timer_script() -> &'static str {
    r#"(function(){
        function parseTime(s){
            let total=0;
            const h=s.match(/(\d+)h/);
            const m=s.match(/(\d+)m/);
            const sec=s.match(/(\d+)s/);
            if(h)total+=parseInt(h[1])*3600;
            if(m)total+=parseInt(m[1])*60;
            if(sec)total+=parseInt(sec[1]);
            return total;
        }
        function fmt(s){
            const h=Math.floor(s/3600);
            const m=Math.floor((s%3600)/60);
            const sec=s%60;
            if(h>0)return`${h}h ${m}m ${sec}s`;
            if(m>0)return`${m}m ${sec}s`;
            return`${sec}s`;
        }
        const timers=new Map();
        document.querySelectorAll('.card-timer').forEach(el=>{
            const txt=el.textContent.trim();
            if(txt&&txt!=='Running'){
                const secs=parseTime(txt);
                if(secs>0)timers.set(el,secs);
            }
        });
        setInterval(()=>{
            timers.forEach((secs,el)=>{
                secs=Math.max(0,secs-1);
                timers.set(el,secs);
                if(secs>0){
                    el.textContent=fmt(secs);
                }else{
                    el.textContent='Expired';
                }
            });
        },1000);
        setTimeout(()=>window.location.reload(),30000);
    })();"#
}
