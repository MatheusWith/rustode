/* Throwaway spike tool: types an ASCII string through libei, with the EIS fd obtained from
 * org.kde.KWin.EIS.RemoteDesktop.connectToEIS on the session bus. Refuses to run unless the
 * session bus is not the real one and WAYLAND_DISPLAY is a qa-virt-* socket. */
#define _GNU_SOURCE
#include <libei.h>
#include <systemd/sd-bus.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <unistd.h>
#include <time.h>
static double now_ms(void){struct timespec t;clock_gettime(CLOCK_REALTIME,&t);return t.tv_sec*1e3+t.tv_nsec/1e6;}
static int code(char c,int*shift){const char*L="qwertyuiop",*M="asdfghjkl",*N="zxcvbnm";char lc=tolower(c);const char*p;*shift=isupper(c);
 if((p=strchr(L,lc))&&lc)return 16+(p-L); if((p=strchr(M,lc))&&lc)return 30+(p-M); if((p=strchr(N,lc))&&lc)return 44+(p-N);
 if(c>='1'&&c<='9')return 2+(c-'1'); if(c=='0')return 11; if(c==' ')return 57; if(c=='\n')return 28; return -1;}
int main(int argc,char**argv){
 const char*wd=getenv("WAYLAND_DISPLAY"),*bus_addr=getenv("DBUS_SESSION_BUS_ADDRESS");
 if(!wd||strncmp(wd,"qa-virt-",8)){fprintf(stderr,"refusing: WAYLAND_DISPLAY=%s\n",wd?wd:"(unset)");return 2;}
 if(!bus_addr||strstr(bus_addr,"/run/user/")){fprintf(stderr,"refusing: session bus %s looks like the real one\n",bus_addr?bus_addr:"(unset)");return 2;}
 if(argc<2){fprintf(stderr,"usage: eistype <text> [delay_ms] [caps]\n");return 1;}
 int delay=argc>2?atoi(argv[2]):30, caps=argc>3?atoi(argv[3]):1;
 sd_bus*bus=NULL;sd_bus_error err=SD_BUS_ERROR_NULL;sd_bus_message*rep=NULL;
 if(sd_bus_open_user(&bus)<0){fprintf(stderr,"no bus\n");return 3;}
 int r=sd_bus_call_method(bus,"org.kde.KWin","/org/kde/KWin/EIS/RemoteDesktop","org.kde.KWin.EIS.RemoteDesktop","connectToEIS",&err,&rep,"i",caps);
 if(r<0){fprintf(stderr,"connectToEIS failed: %s %s\n",err.name,err.message);return 3;}
 int fd,cookie; sd_bus_message_read(rep,"hi",&fd,&cookie); fd=dup(fd);
 fprintf(stderr,"connectToEIS(caps=%d) -> fd %d cookie %d on bus %s\n",caps,fd,cookie,bus_addr);
 struct ei*ei=ei_new_sender(NULL); ei_configure_name(ei,"rustode-spike");
 if(ei_setup_backend_fd(ei,fd)<0){fprintf(stderr,"setup_backend_fd failed\n");return 4;}
 struct ei_device*kbd=NULL;int ready=0;unsigned seq=1;
 double t0=now_ms();
 while(!ready&&now_ms()-t0<5000){
  struct pollfd p={ei_get_fd(ei),POLLIN,0}; poll(&p,1,200); ei_dispatch(ei);
  struct ei_event*e;
  while((e=ei_get_event(ei))){
   switch(ei_event_get_type(e)){
    case EI_EVENT_CONNECT: fprintf(stderr,"EI connected\n");break;
    case EI_EVENT_DISCONNECT: fprintf(stderr,"EI disconnected\n");return 5;
    case EI_EVENT_SEAT_ADDED:{struct ei_seat*s=ei_event_get_seat(e);fprintf(stderr,"seat %s kbd=%d\n",ei_seat_get_name(s),ei_seat_has_capability(s,EI_DEVICE_CAP_KEYBOARD));
      ei_seat_bind_capabilities(s,EI_DEVICE_CAP_KEYBOARD,NULL);break;}
    case EI_EVENT_DEVICE_ADDED:{struct ei_device*d=ei_event_get_device(e);fprintf(stderr,"device %s kbd=%d\n",ei_device_get_name(d),ei_device_has_capability(d,EI_DEVICE_CAP_KEYBOARD));
      if(ei_device_has_capability(d,EI_DEVICE_CAP_KEYBOARD))kbd=ei_device_ref(d);break;}
    case EI_EVENT_DEVICE_RESUMED: if(ei_event_get_device(e)==kbd){ei_device_start_emulating(kbd,seq++);ready=1;fprintf(stderr,"keyboard resumed\n");}break;
    default:break;}
   ei_event_unref(e);}
 }
 if(!ready){fprintf(stderr,"no keyboard device resumed within 5 s\n");return 6;}
 for(const char*c=argv[1];*c;c++){int sh;int k=code(*c,&sh);if(k<0)continue;
  if(sh){ei_device_keyboard_key(kbd,42,true);ei_device_frame(kbd,ei_now(ei));}
  printf("SEND %.3f '%c' code=%d\n",now_ms(),*c,k);fflush(stdout);
  ei_device_keyboard_key(kbd,k,true);ei_device_frame(kbd,ei_now(ei));
  ei_device_keyboard_key(kbd,k,false);ei_device_frame(kbd,ei_now(ei));
  if(sh){ei_device_keyboard_key(kbd,42,false);ei_device_frame(kbd,ei_now(ei));}
  ei_dispatch(ei); usleep(delay*1000);}
 ei_device_stop_emulating(kbd); ei_dispatch(ei); usleep(100000);
 fprintf(stderr,"done\n"); ei_device_unref(kbd); ei_unref(ei); return 0;}
