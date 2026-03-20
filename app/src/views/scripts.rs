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
